use std::fs;
use std::path::{Path, PathBuf};

use crate::Diagnostics::{Diagnostic, Severity};
use crate::SHA256;

use super::graph_projection::project_checked;
use super::project_transactions::{diagnostic_json, rel_path};
use super::schema_api::{Projection, source_revision};
use super::validation_json::{json_optional_str, json_str};

pub(super) fn project_file(path: &Path) -> Result<Projection, Vec<Diagnostic>> {
    let path_str = path.to_string_lossy();
    let src = fs::read_to_string(path).unwrap_or_default();
    let (diags, bundle, facts) = crate::Driver::check_file_with_effect_facts(&path_str, None, true);
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
    Ok(project_checked(path, &src, &bundle, &facts))
}

#[derive(Clone)]
pub(super) struct ProjectFileRec {
    path: String,
    revision: String,
    kind: String,
}

pub(super) struct ProjectContext {
    entry_path: PathBuf,
    project_root: PathBuf,
    manifest_root: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
    files: Vec<ProjectFileRec>,
    project_revision: String,
}

pub(super) struct TouchedProjectFile {
    path: String,
    revision: String,
}

pub(super) struct ProjectChange {
    path: PathBuf,
    rel: String,
    before: String,
    after: String,
}

pub(super) fn project_context_for_entry(path: &Path) -> ProjectContext {
    let entry_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let manifest_root = crate::Loader::find_manifest_root(entry_dir);
    let workspace_root = find_workspace_root(manifest_root.as_deref().unwrap_or(entry_dir));
    let project_root = workspace_root
        .as_deref()
        .or(manifest_root.as_deref())
        .unwrap_or(entry_dir)
        .to_path_buf();
    let files = collect_project_files(
        &project_root,
        path,
        manifest_root.as_deref(),
        workspace_root.as_deref(),
    );
    let project_revision = project_revision_from_files(&files);
    ProjectContext {
        entry_path: path.to_path_buf(),
        project_root,
        manifest_root,
        workspace_root,
        files,
        project_revision,
    }
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if crate::Jetpack::WorkspaceFile::load(&dir).is_some()
            || crate::Jetpack::WorkspaceLock::load(&dir).is_some()
        {
            return Some(dir);
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return None,
        }
    }
}

fn collect_project_files(
    project_root: &Path,
    entry_path: &Path,
    manifest_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Vec<ProjectFileRec> {
    let mut paths = Vec::new();
    push_existing(&mut paths, entry_path);
    push_existing(&mut paths, &project_root.join(crate::Syntax::ENV_FILE));
    if let Some(root) = manifest_root {
        push_existing(&mut paths, &root.join(crate::Syntax::PAYLOAD_FILE));
    }
    if let Some(root) = workspace_root {
        push_existing(&mut paths, &root.join(crate::Syntax::WORKSPACE_FILE));
        push_existing(&mut paths, &root.join(crate::Syntax::UNIFIED_LOCK_FILE));
        if let Some(Ok(plan)) = crate::Jetpack::WorkspaceFile::load(root) {
            for member in plan.members {
                let member_dir = root.join(member.path);
                push_existing(&mut paths, &member_dir.join(crate::Syntax::PAYLOAD_FILE));
                collect_jet_files(&member_dir, &mut paths);
            }
        }
    }
    if workspace_root.is_none() {
        if let Some(root) = manifest_root {
            collect_jet_files(root, &mut paths);
        }
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| {
            let bytes = fs::read(&path).ok()?;
            let kind = if path.file_name().and_then(|n| n.to_str())
                == Some(crate::Syntax::PAYLOAD_FILE)
            {
                "manifest"
            } else if path.file_name().and_then(|n| n.to_str())
                == Some(crate::Syntax::WORKSPACE_FILE)
            {
                "workspace"
            } else if path.file_name().and_then(|n| n.to_str()) == Some(crate::Syntax::ENV_FILE) {
                "env"
            } else if rel_path(project_root, &path) == crate::Syntax::UNIFIED_LOCK_FILE {
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
        if path.extension().and_then(|e| e.to_str()) == Some(crate::Syntax::FILE_EXT)
            && path.file_name().and_then(|n| n.to_str()) != Some(crate::Syntax::PAYLOAD_FILE)
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
    let members = match crate::Jetpack::WorkspaceFile::load(root) {
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
                json_str(&rel_path(project_root, &root.join(crate::Syntax::WORKSPACE_FILE))),
                diagnostic_json(&d)
            );
        }
        None => String::new(),
    };
    format!(
        "{{\"path\":{},\"members\":[{}],\"diagnostics\":[]}}",
        json_str(&rel_path(project_root, &root.join(crate::Syntax::WORKSPACE_FILE))),
        members
    )
}

pub(super) fn packages_project_json(
    project_root: &Path,
    manifest_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> String {
    package_dirs(manifest_root, workspace_root)
        .iter()
        .filter_map(|dir| package_project_json(project_root, dir))
        .collect::<Vec<_>>()
        .join(",")
}

fn package_dirs(manifest_root: Option<&Path>, workspace_root: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = manifest_root {
        dirs.push(root.to_path_buf());
    }
    if let Some(root) = workspace_root {
        if let Some(Ok(plan)) = crate::Jetpack::WorkspaceFile::load(root) {
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
    workspace_root: Option<&Path>,
) -> String {
    package_dirs(manifest_root, workspace_root)
        .iter()
        .filter_map(|dir| package_targets_project_json(project_root, dir))
        .flatten()
        .collect::<Vec<_>>()
        .join(",")
}

fn package_targets_project_json(project_root: &Path, dir: &Path) -> Option<Vec<String>> {
    let manifest_path = dir.join(crate::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&manifest_path).ok()?;
    let manifest = crate::Jetpack::PackageManifest::parse(&raw).ok()?;
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

fn package_project_json(project_root: &Path, dir: &Path) -> Option<String> {
    let manifest_path = dir.join(crate::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&manifest_path).ok()?;
    match crate::Jetpack::PackageManifest::parse(&raw) {
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

struct EnvProjectJson {
    envs: String,
    services: String,
    diagnostics: String,
}

pub(super) fn env_project_json(project_root: &Path) -> EnvProjectJson {
    let path = project_root.join(crate::Syntax::ENV_FILE);
    let Ok(src) = fs::read_to_string(&path) else {
        return EnvProjectJson {
            envs: String::new(),
            services: String::new(),
            diagnostics: String::new(),
        };
    };
    match crate::Jetpack::ModuleEval::evaluate_env(&src, project_root) {
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
            EnvProjectJson {
                envs: format!(
                    "{{\"path\":{},\"prompt\":{},\"packages\":[{}],\"secrets\":[{}],\"diagnostics\":[]}}",
                    json_str(crate::Syntax::ENV_FILE),
                    json_str(plan.prompt.as_deref().unwrap_or(crate::Syntax::JETPACK_PROMPT_LABEL)),
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

fn dev_service_project_json(service: &crate::Jetpack::ModuleEval::DevServicePlan) -> String {
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
    format!(
        "{{\"name\":{},\"enable\":{},\"ports\":[{}],\"init\":{},\"shutdown\":{},\"data_dir\":{},\"ready\":{},\"extra\":[{}],\"source\":{}}}",
        json_str(&service.name),
        if service.enable { "true" } else { "false" },
        ports,
        json_optional_str(service.init.as_deref()),
        json_optional_str(service.shutdown.as_deref()),
        json_optional_str(service.data_dir.as_deref()),
        json_optional_str(service.ready.as_deref()),
        extra,
        json_str(crate::Syntax::ENV_FILE)
    )
}

pub(super) fn lock_project_json(project_root: &Path) -> String {
    let lock_path = project_root.join(crate::Syntax::UNIFIED_LOCK_FILE);
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

fn dep_source_label(source: &crate::Jetpack::PackageManifest::DepSource) -> String {
    match source {
        crate::Jetpack::PackageManifest::DepSource::Version(v) => format!("version:{v}"),
        crate::Jetpack::PackageManifest::DepSource::Provider { provider, target } => {
            format!("{provider:?}@{target}")
        }
        crate::Jetpack::PackageManifest::DepSource::Git { url, selector } => {
            format!("git:{url}@{selector:?}")
        }
        crate::Jetpack::PackageManifest::DepSource::CLib { target } => {
            format!("c:{target}")
        }
    }
}

fn target_label(target: &crate::Jetpack::PackageManifest::Target) -> String {
    match target {
        crate::Jetpack::PackageManifest::Target::Library => "library".to_string(),
        crate::Jetpack::PackageManifest::Target::Executable => "executable".to_string(),
        crate::Jetpack::PackageManifest::Target::Test => "test".to_string(),
        crate::Jetpack::PackageManifest::Target::Example => "example".to_string(),
        crate::Jetpack::PackageManifest::Target::Benchmark => "benchmark".to_string(),
        crate::Jetpack::PackageManifest::Target::Plugin { .. } => "plugin".to_string(),
    }
}
