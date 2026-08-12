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
    let (diags, bundle, facts) = jet_driver::Driver::check_file_with_effect_facts(
        &path_str,
        Some((&checked.path, &src)),
        true,
    );
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
    resolver
        .revalidate_file(&checked)
        .map_err(|error| vec![error.diagnostic()])?;
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
    let (workspace_ecosystem_root, ecosystem_diagnostic) = match workspace_root {
        Some(root) => match AuthorityResolver::open(root) {
            Ok(resolver) => match resolver.checked_manifest(Path::new(".")) {
                Ok(manifest) => match resolver.revalidate_file(&manifest.file) {
                    Ok(()) => (Some(root.to_path_buf()), None),
                    Err(error) => (None, Some(error.diagnostic())),
                },
                Err(error) if error.is_missing() => (None, None),
                Err(error) => (None, Some(error.diagnostic())),
            },
            Err(error) if error.is_missing() => (None, None),
            Err(error) => (None, Some(error.diagnostic())),
        },
        None => (None, None),
    };
    let ecosystem_root = workspace_ecosystem_root.or_else(|| manifest_root.clone());
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
        authority_diagnostic: collection_diagnostic
            .or(manifest_diagnostic)
            .or(ecosystem_diagnostic)
            .or_else(|| {
            workspace_boundary
                .as_ref()
                .and_then(|boundary| boundary.diagnostic.clone())
        }),
    }
}

fn find_workspace_boundary(start: &Path) -> Option<WorkspaceBoundary> {
    let mut dir = match AuthorityResolver::open(start) {
        Ok(resolver) => resolver.root().to_path_buf(),
        Err(error) if error.is_missing() => return None,
        Err(error) => {
            return Some(WorkspaceBoundary {
                root: start.to_path_buf(),
                member_root: None,
                malformed: true,
                diagnostic: Some(error.diagnostic()),
            })
        }
    };
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
        dir = resolver.root().to_path_buf();
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
    let entry_resolver = AuthorityResolver::open(entry_dir)
        .map_err(|error| error.diagnostic())?;
    let entry_path = entry_resolver.root().to_path_buf();
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
    let result = matches
        .into_iter()
        .filter(|member| entry_path.starts_with(&member.path))
        .map(|member| member.path)
        .last();
    resolver
        .revalidate_root()
        .map_err(|error| error.diagnostic())?;
    Ok(result)
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
    AuthorityResolver::open(path)
        .ok()
        .map(|resolver| resolver.root().to_path_buf())
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
    let mut workspace_source_failed = false;
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
                workspace_source_failed = true;
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
        if let Some((_, source)) = workspace_source.as_ref() {
            push_existing(&mut paths, &source.path);
        }
        push_existing(&mut paths, &root.join(jet_driver::Syntax::UNIFIED_LOCK_FILE));
        // A failed source resolution is already an authority diagnostic. Do
        // not reopen `workspace.jet` as a fallback authority path.
        if let Some((source_resolver, source)) = workspace_source.as_ref() {
            if source.role == jet_pkg_model::WorkspacePlan::WorkspaceSourceRole::Index {
                match workspace_snapshot_for_source(source_resolver, source) {
                    Ok(snapshot) => {
                        for member in snapshot.plan.members {
                            let member_dir = root.join(member.path);
                            push_existing(&mut paths, &member_dir.join(jet_driver::Syntax::PACKAGE_FILE));
                        }
                    }
                    Err(diagnostic) => authority_diagnostic = Some(diagnostic),
                }
            }
        }
    }
    if let Some(resolver) = resolver.as_ref() {
        match resolver.discover_source_files() {
            Ok(source_files) => paths.extend(source_files.into_iter().filter_map(|file| {
                let is_fallback_workspace = workspace_source_failed
                    && workspace_root.is_some_and(|root| {
                        file.path == root.join(jet_driver::Syntax::WORKSPACE_FILE)
                    });
                (!is_fallback_workspace).then_some(file.path)
            })),
            Err(error) => authority_diagnostic.get_or_insert_with(|| error.diagnostic()),
        }
    }
    let authority_root = resolver
        .as_ref()
        .map(|resolver| resolver.root())
        .unwrap_or(project_root);
    paths.sort();
    paths.dedup();
    let mut files = Vec::new();
    for path in paths {
        let Some(resolver) = resolver.as_ref() else {
            break;
        };
        let relative = match path.strip_prefix(authority_root) {
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
        let bytes = checked.bytes.clone();
            let kind = if path.file_name().and_then(|n| n.to_str()) == Some(jet_driver::Syntax::PACKAGE_FILE) {
                "package"
            } else if workspace_source
                .as_ref()
                .is_some_and(|(_, source)| path.as_path() == source.path.as_path())
            {
                "workspace"
            } else if path.file_name().and_then(|n| n.to_str()) == Some(jet_driver::Syntax::ENV_FILE) {
                "env"
            } else if rel_path(authority_root, &path) == jet_driver::Syntax::UNIFIED_LOCK_FILE {
                "lock"
            } else {
                "source"
            };
        if let Err(error) = resolver.revalidate_file(&checked) {
            authority_diagnostic.get_or_insert_with(|| error.diagnostic());
            continue;
        }
        files.push(ProjectFileRec {
                path: rel_path(authority_root, &path),
                revision: format!("sha256-{}", SHA256::sha256_hex(&bytes)),
                kind: kind.to_string(),
        });
    }
    (files, authority_diagnostic)
}

fn workspace_source_path(
    root: &Path,
) -> Result<
    Option<(
        AuthorityResolver,
        jet_pkg_model::WorkspacePlan::WorkspaceSource,
    )>,
    Diagnostic,
> {
    let resolver = AuthorityResolver::open(root).map_err(|error| error.diagnostic())?;
    match resolver
        .resolve_workspace_source()
        .map_err(|error| error.workspace_diagnostic())?
    {
        Some(source) => Ok(Some((resolver, source))),
        None => Ok(None),
    }
}

fn workspace_snapshot_for_source(
    resolver: &AuthorityResolver,
    source: &jet_pkg_model::WorkspacePlan::WorkspaceSource,
) -> Result<jet_env_model::WorkspaceFile::WorkspaceSnapshot, Diagnostic> {
    jet_env_model::WorkspaceFile::load_checked_source(resolver, source.clone())
}

fn push_existing(paths: &mut Vec<PathBuf>, path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(_) => paths.push(path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => paths.push(path.to_path_buf()),
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
    let source = match workspace_source_path(root) {
        Ok(source) => source,
        Err(diagnostic) => {
            return format!(
                "{{\"path\":\"\",\"members\":[],\"diagnostics\":[{}]}}",
                diagnostic_json(&diagnostic)
            );
        }
    };
    let source_path = source
        .as_ref()
        .map(|(_, source)| rel_path(project_root, &source.path))
        .unwrap_or_default();
    let members = match source.as_ref() {
        Some((resolver, source))
            if source.role
                == jet_pkg_model::WorkspacePlan::WorkspaceSourceRole::Index => {
            match workspace_snapshot_for_source(resolver, source) {
                Ok(snapshot) => snapshot
                    .plan
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
                Err(diagnostic) => {
                    return format!(
                        "{{\"path\":{},\"members\":[],\"diagnostics\":[{}]}}",
                        json_str(&source_path),
                        diagnostic_json(&diagnostic)
                    );
                }
            }
        }
        Some(_) | None => String::new(),
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
    let dirs = match package_dirs(manifest_root, ecosystem_root, workspace_root) {
        Ok(dirs) => dirs,
        Err(diagnostic) => return projection_diagnostic_json(&diagnostic),
    };
    dirs
        .iter()
        .filter_map(|dir| package_project_json(project_root, entry_path, dir))
        .collect::<Vec<_>>()
        .join(",")
}

fn package_dirs(
    manifest_root: Option<&Path>,
    ecosystem_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut dirs = Vec::new();
    if let Some(root) = manifest_root {
        dirs.push(root.to_path_buf());
    }
    if let Some(root) = ecosystem_root {
        dirs.push(root.to_path_buf());
    }
    if let Some(root) = workspace_root {
        if let Some((resolver, source)) = workspace_source_path(root)? {
            if source.role == jet_pkg_model::WorkspacePlan::WorkspaceSourceRole::Index {
                let snapshot = workspace_snapshot_for_source(&resolver, &source)?;
                for member in snapshot.plan.members {
                    dirs.push(root.join(member.path));
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    Ok(dirs)
}

pub(super) fn targets_project_json(
    project_root: &Path,
    entry_path: &Path,
    manifest_root: Option<&Path>,
    ecosystem_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> String {
    let dirs = match package_dirs(manifest_root, ecosystem_root, workspace_root) {
        Ok(dirs) => dirs,
        Err(diagnostic) => return projection_diagnostic_json(&diagnostic),
    };
    dirs
        .iter()
        .filter_map(|dir| package_targets_project_json(project_root, entry_path, dir))
        .flatten()
        .collect::<Vec<_>>()
        .join(",")
}

fn package_targets_project_json(
    project_root: &Path,
    entry_path: &Path,
    dir: &Path,
) -> Option<Vec<String>> {
    canonical_package_targets_project_json(project_root, entry_path, dir)
}

fn package_project_json(project_root: &Path, entry_path: &Path, dir: &Path) -> Option<String> {
    canonical_package_project_json(project_root, entry_path, dir)
}

fn canonical_package_facts(
    dir: &Path,
) -> Result<jet_driver::Package::PackageFacts, String> {
    jet_driver::Package::PackageFacts::load_checked(dir)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "canonical Package `{}` is missing",
                dir.join(jet_driver::Syntax::PACKAGE_FILE).display()
            )
        })
}

fn projection_diagnostic_json(diagnostic: &Diagnostic) -> String {
    format!("{{\"diagnostics\":[{}]}}", diagnostic_json(diagnostic))
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

fn canonical_package_targets_project_json(
    project_root: &Path,
    entry_path: &Path,
    dir: &Path,
) -> Option<Vec<String>> {
    let facts = match canonical_package_facts(dir) {
        Ok(facts) => facts,
        Err(error) => return Some(vec![canonical_package_error(project_root, entry_path, dir, &error)]),
    };
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
    let resolver = match AuthorityResolver::open(project_root) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => {
            return EnvProjectJson {
                envs: String::new(),
                services: String::new(),
                diagnostics: String::new(),
            }
        }
        Err(error) => {
            return EnvProjectJson {
                envs: String::new(),
                services: String::new(),
                diagnostics: diagnostic_json(&error.diagnostic()),
            }
        }
    };
    let checked = match resolver.checked_file(Path::new(jet_driver::Syntax::ENV_FILE)) {
        Ok(file) => file,
        Err(error) if error.is_missing() => {
            return EnvProjectJson {
                envs: String::new(),
                services: String::new(),
                diagnostics: String::new(),
            }
        }
        Err(error) => {
            return EnvProjectJson {
                envs: String::new(),
                services: String::new(),
                diagnostics: diagnostic_json(&error.diagnostic()),
            }
        }
    };
    let src = match checked.text() {
        Ok(src) => src,
        Err(error) => {
            return EnvProjectJson {
                envs: String::new(),
                services: String::new(),
                diagnostics: diagnostic_json(&error.diagnostic()),
            }
        }
    };
    if let Err(error) = resolver.revalidate_file(&checked) {
        return EnvProjectJson {
            envs: String::new(),
            services: String::new(),
            diagnostics: diagnostic_json(&error.diagnostic()),
        };
    }
    let plan = match jet_env_model::ModuleEval::evaluate_env(&src, project_root) {
        Ok(plan) => plan,
        Err(d) => {
            return EnvProjectJson {
                envs: String::new(),
                services: String::new(),
                diagnostics: diagnostic_json(&d),
            }
        }
    };
    if let Err(error) = resolver.revalidate_file(&checked) {
        return EnvProjectJson {
            envs: String::new(),
            services: String::new(),
            diagnostics: diagnostic_json(&error.diagnostic()),
        };
    }
    {
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
    let Ok(resolver) = AuthorityResolver::open(project_root) else {
        return String::new();
    };
    let Ok(lock) = resolver.checked_file(Path::new(jet_driver::Syntax::UNIFIED_LOCK_FILE)) else {
        return String::new();
    };
    let revision = format!("sha256-{}", SHA256::sha256_hex(&lock.bytes));
    if resolver.revalidate_file(&lock).is_err() {
        return String::new();
    }
    format!(
        "{{\"path\":{},\"revision\":{},\"kind\":\"unified\"}}",
        json_str(&rel_path(project_root, &lock.path)),
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
