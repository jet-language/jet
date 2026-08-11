use std::fs;
use std::path::{Path, PathBuf};

use jet_driver::Diagnostics::{Diagnostic, Severity};
use jet_driver::SHA256;
use jet_pkg_model::Authority::AuthorityResolver;

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
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = AuthorityResolver::open(root)
        .map_err(|error| vec![error.diagnostic()])?;
    let Some(file_name) = path.file_name() else {
        return Err(vec![jet_pkg_model::Authority::AuthorityError::Invalid {
            path: path.to_path_buf(),
            detail: "source entry has no file name".to_string(),
        }
        .diagnostic()]);
    };
    let checked = resolver
        .checked_file(file_name)
        .map_err(|error| vec![error.diagnostic()])?;
    resolver
        .revalidate_file(&checked)
        .map_err(|error| vec![error.diagnostic()])?;
    let src = checked.text().map_err(|error| vec![error.diagnostic()])?;
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
    /// Package root used by Canvas package projections. It comes from the
    /// loader's one root walk; workspace roots still take precedence.
    pub(super) ecosystem_root: Option<PathBuf>,
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) files: Vec<ProjectFileRec>,
    pub(super) parts: jet_driver::ProjectParts::ProjectPartsReport,
    pub(super) project_revision: String,
    pub(super) authority_diagnostic: Option<Diagnostic>,
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
    diagnostic: Option<Diagnostic>,
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
    let (manifest_root, manifest_diagnostic) = match jet_driver::Loader::find_manifest_root_checked(entry_dir) {
        Ok(root) => (root, None),
        Err(diagnostic) => (None, Some(diagnostic)),
    };
    let manifest_root = manifest_root.filter(|manifest| {
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
        .filter(|root| {
            AuthorityResolver::open(root).is_ok_and(|resolver| {
                resolver
                    .checked_manifest(Path::new("."))
                    .is_ok()
            })
        })
        .map(Path::to_path_buf)
        .or_else(|| manifest_root.clone());
    let project_root = workspace_root
        .as_deref()
        .or(manifest_root.as_deref())
        .or(ecosystem_root.as_deref())
        .unwrap_or(entry_dir)
        .to_path_buf();
    let (files, collection_diagnostic) = collect_project_files(
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
        // File inventory is also an authority read. Keep its error visible to
        // the caller instead of turning a failed read into an empty project.
        authority_diagnostic: collection_diagnostic.or(manifest_diagnostic).or_else(|| {
            workspace_boundary
                .as_ref()
                .and_then(|boundary| boundary.diagnostic.clone())
        }),
    }
}

fn find_workspace_boundary(start: &Path) -> Option<WorkspaceBoundary> {
    let mut dir = start.to_path_buf();
    loop {
        let resolver = match AuthorityResolver::open(&dir) {
            Ok(resolver) => resolver,
            Err(error) if error.is_missing() => {
                match dir.parent() {
                    Some(parent) => {
                        dir = parent.to_path_buf();
                        continue;
                    }
                    None => return None,
                }
            }
            Err(error) => {
                return Some(WorkspaceBoundary {
                    root: dir,
                    member_root: None,
                    malformed: true,
                    diagnostic: Some(error.diagnostic()),
                })
            }
        };
        match resolver.resolve_workspace_source() {
            Ok(Some(source)) => {
                let evaluation = resolver
                    .revalidate_source(&source)
                    .map_err(|error| error.diagnostic())
                    .and_then(|_| {
                        jet_env_model::WorkspaceFile::evaluate_checked_source(
                            &source,
                            &resolver,
                        )
                    });
                let (plan, diagnostic) = match evaluation {
                    Ok(plan) => match resolver.revalidate_source(&source) {
                        Ok(()) => (Some(plan), None),
                        Err(error) => (None, Some(error.diagnostic())),
                    },
                    Err(diagnostic) => (None, Some(diagnostic)),
                };
                if source.role == jet_env_model::WorkspaceFile::WorkspaceSourceRole::Authority {
                    return Some(WorkspaceBoundary {
                        root: dir,
                        member_root: None,
                        malformed: diagnostic.is_some(),
                        diagnostic,
                    });
                }
                let Some(plan) = plan else {
                    return Some(WorkspaceBoundary {
                        root: dir,
                        member_root: None,
                        malformed: true,
                        diagnostic,
                    })
                };
                let member_root = match matching_member_root(&resolver, start, &plan) {
                    Ok(member_root) => member_root,
                    Err(diagnostic) => {
                        return Some(WorkspaceBoundary {
                            root: dir,
                            member_root: None,
                            malformed: true,
                            diagnostic: Some(diagnostic),
                        })
                    }
                };
                return Some(WorkspaceBoundary {
                    root: dir,
                    member_root,
                    malformed: false,
                    diagnostic: None,
                });
            }
            Ok(None) => {}
            Err(error) => {
                return Some(WorkspaceBoundary {
                    root: dir,
                    member_root: None,
                    malformed: true,
                    diagnostic: Some(error.workspace_diagnostic()),
                })
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return None,
        }
    }
}

fn matching_member_root(
    resolver: &AuthorityResolver,
    entry_dir: &Path,
    plan: &jet_env_model::WorkspaceFile::WorkspacePlan,
) -> Result<Option<PathBuf>, Diagnostic> {
    let entry = resolver
        .checked_directory(entry_dir)
        .map_err(|error| error.diagnostic())?;
    resolver
        .revalidate_directory(&entry)
        .map_err(|error| error.diagnostic())?;
    let mut matches = plan
        .members
        .iter()
        .map(|member| {
            resolver
                .checked_directory(Path::new(&member.path))
                .map_err(|error| error.diagnostic())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for member in &matches {
        resolver
            .revalidate_directory(member)
            .map_err(|error| error.diagnostic())?;
    }
    matches.sort_by_key(|member| member.path.components().count());
    Ok(matches
        .into_iter()
        .filter(|member| entry.path.starts_with(&member.path))
        .map(|member| member.path)
        .last())
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    comparable_path(path)
        .zip(comparable_path(root))
        .is_some_and(|(path, root)| path.starts_with(root))
}

fn same_path(left: &Path, right: &Path) -> bool {
    comparable_path(left)
        .zip(comparable_path(right))
        .is_some_and(|(left, right)| left == right)
}

fn comparable_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn collect_project_files(
    project_root: &Path,
    entry_path: &Path,
    manifest_root: Option<&Path>,
    ecosystem_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> (Vec<ProjectFileRec>, Option<Diagnostic>) {
    let mut paths = Vec::new();
    let mut authority_diagnostic = None;
    let resolver = match AuthorityResolver::open(project_root) {
        Ok(resolver) => Some(resolver),
        Err(error) => {
            authority_diagnostic = Some(error.diagnostic());
            None
        }
    };
    let workspace_source = if let Some(root) = workspace_root {
        match workspace_source_path(root) {
            Ok(source) => source,
            Err(diagnostic) => {
                authority_diagnostic = Some(diagnostic);
                None
            }
        }
    } else {
        None
    };
    push_existing(&mut paths, entry_path);
    push_existing(&mut paths, &project_root.join(jet_driver::Syntax::ENV_FILE));
    if let Some(root) = manifest_root {
        push_existing(&mut paths, &root.join(jet_driver::Syntax::PACKAGE_FILE));
    }
    if let Some(root) = ecosystem_root {
        push_existing(&mut paths, &root.join(jet_driver::Syntax::PACKAGE_FILE));
    }
    if let Some(root) = workspace_root {
        if let Some(source) = workspace_source.clone() {
            push_existing(&mut paths, &source);
        }
        push_existing(&mut paths, &root.join(jet_driver::Syntax::UNIFIED_LOCK_FILE));
        if let Some(result) = jet_env_model::WorkspaceFile::load_checked(root) {
            match result {
                Ok(snapshot) => {
                    for member in snapshot.plan.members {
                        let member_dir = root.join(member.path);
                        push_existing(&mut paths, &member_dir.join(jet_driver::Syntax::PACKAGE_FILE));
                        collect_jet_files(&member_dir, &mut paths);
                    }
                }
                Err(diagnostic) => authority_diagnostic = Some(diagnostic),
            }
        }
    }
    if workspace_root.is_none() {
        collect_jet_files(manifest_root.unwrap_or(project_root), &mut paths);
    }
    paths.sort();
    paths.dedup();
    let mut files = Vec::new();
    for path in paths {
        let Some(resolver) = resolver.as_ref() else {
            break;
        };
        let relative = match path.strip_prefix(project_root) {
            Ok(relative) => relative,
            Err(_) => {
                authority_diagnostic.get_or_insert_with(|| {
                    jet_pkg_model::Authority::AuthorityError::Escapes(path.clone()).diagnostic()
                });
                continue;
            }
        };
        let checked = match resolver.checked_file(relative) {
            Ok(checked) => checked,
            Err(error) => {
                authority_diagnostic.get_or_insert_with(|| error.diagnostic());
                continue;
            }
        };
        if let Err(error) = resolver.revalidate_file(&checked) {
            authority_diagnostic.get_or_insert_with(|| error.diagnostic());
            continue;
        }
        let bytes = checked.bytes;
            let kind = if path.file_name().and_then(|n| n.to_str()) == Some(jet_driver::Syntax::PACKAGE_FILE) {
                "package"
            } else if workspace_source
                .as_ref()
                .is_some_and(|source| path.as_path() == source.as_path())
                "workspace"
            } else if path.file_name().and_then(|n| n.to_str()) == Some(jet_driver::Syntax::ENV_FILE) {
                "env"
            } else if rel_path(project_root, &path) == jet_driver::Syntax::UNIFIED_LOCK_FILE {
                "lock"
            } else {
                "source"
            };
            files.push(ProjectFileRec {
                path: rel_path(project_root, &path),
                revision: format!("sha256-{}", SHA256::sha256_hex(&bytes)),
                kind: kind.to_string(),
            });
    }
    (files, authority_diagnostic)
}

fn workspace_source_path(root: &Path) -> Result<Option<PathBuf>, Diagnostic> {
    let resolver = AuthorityResolver::open(root).map_err(|error| error.diagnostic())?;
    resolver
        .resolve_workspace_source()
        .map(|source| source.map(|source| source.path))
        .map_err(|error| error.workspace_diagnostic())
}

fn push_existing(paths: &mut Vec<PathBuf>, path: &Path) {
    if fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
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
    let source_path = match workspace_source_path(root) {
        Ok(Some(path)) => rel_path(project_root, &path),
        Ok(None) => String::new(),
        Err(diagnostic) => {
            return format!(
                "{{\"path\":\"\",\"members\":[],\"diagnostics\":[{}]}}",
                diagnostic_json(&diagnostic)
            );
        }
    };
    let members = match jet_env_model::WorkspaceFile::load_checked(root) {
        Some(Ok(snapshot)) => snapshot.plan
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
                json_str(&source_path),
                diagnostic_json(&d)
            );
        }
        None => String::new(),
    };
    format!(
        "{{\"path\":{},\"members\":[{}],\"diagnostics\":[]}}",
        json_str(&source_path),
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
        if let Some(result) = jet_env_model::WorkspaceFile::load_checked(root) {
            if let Ok(snapshot) = result {
                for member in snapshot.plan.members {
                    dirs.push(root.join(member.path));
                }
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
    canonical_package_targets_project_json(project_root, dir)
}

fn package_project_json(project_root: &Path, entry_path: &Path, dir: &Path) -> Option<String> {
    canonical_package_project_json(project_root, entry_path, dir)
}

fn canonical_package_facts(
    dir: &Path,
) -> Result<jet_driver::Package::PackageFacts, String> {
    jet_driver::Package::PackageFacts::load(dir)
        .ok_or_else(|| format!("canonical Package `{}` is missing", dir.join(jet_driver::Syntax::PACKAGE_FILE).display()))?
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
        json_str(&rel_path(project_root, &dir.join(jet_driver::Syntax::PACKAGE_FILE))),
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
    let manifest_rel = rel_path(project_root, &dir.join(jet_driver::Syntax::PACKAGE_FILE));
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
        jet_semindex::workspace_overlay_policy_for_entry(&dir.join(jet_driver::Syntax::PACKAGE_FILE))
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
                json_str(&jet_driver::Package::dep_display(source))
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(format!(
        "{{\"path\":{},\"manifest\":{},\"name\":{},\"version\":{},\"target\":\"native\",\"deps\":[{}],\"targets\":[{}],\"outputs\":[{}],\"environments\":[{}],\"configs\":[{}],\"members\":[{}],\"package_facts\":{},\"workspace_overlays\":{},\"effects_enabled\":false,\"diagnostics\":{}}}",
        json_str(&rel_path(project_root, dir)),
        json_str(&rel_path(project_root, &dir.join(jet_driver::Syntax::PACKAGE_FILE))),
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
            let presets = plan
                .presets
                .iter()
                .map(|preset| json_str(&preset.name))
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
                    "{{\"path\":{},\"prompt\":{},\"environments\":[{}],\"sources\":[{}],\"presets\":[{}],\"languages\":[{}],\"reload\":{},\"packages\":[{}],\"secrets\":[{}],\"diagnostics\":[]}}",
                    json_str(jet_driver::Syntax::ENV_FILE),
                    json_str(plan.prompt.as_deref().unwrap_or(jet_driver::Syntax::JETPACK_PROMPT_LABEL)),
                    environments,
                    sources,
                    presets,
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

fn dep_source_label(source: &jet_driver::Package::DepSource) -> String {
    match source {
        jet_driver::Package::DepSource::Version(v) => format!("version:{v}"),
        jet_driver::Package::DepSource::Provider { provider, target } => {
            format!("{provider:?}@{target}")
        }
        jet_driver::Package::DepSource::Git { url, selector } => {
            format!("git:{url}@{selector:?}")
        }
        jet_driver::Package::DepSource::CLib { target } => {
            format!("c:{target}")
        }
    }
}

fn target_label(target: &jet_driver::Package::Target) -> String {
    match target {
        jet_driver::Package::Target::Library => "library".to_string(),
        jet_driver::Package::Target::Executable => "executable".to_string(),
        jet_driver::Package::Target::Test => "test".to_string(),
        jet_driver::Package::Target::Example => "example".to_string(),
        jet_driver::Package::Target::Benchmark => "benchmark".to_string(),
        jet_driver::Package::Target::Plugin { .. } => "plugin".to_string(),
    }
}
