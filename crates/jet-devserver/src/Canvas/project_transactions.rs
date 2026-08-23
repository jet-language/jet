use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use jet_driver::Diagnostics::{Diagnostic, Severity};
use jet_driver::FixEngine;
use jet_semindex::{DefinitionAnchor, SourceSpan, SymbolDef, SymbolRef};

use super::graph_helpers::{edit, project_edit_error, project_edit_ok, simple_diff};
use super::project_scan::{
    project_context_for_entry, project_revision_from_files, ProjectChange, ProjectContext,
    ProjectFileRec, TouchedProjectFile,
};
use super::schema_api::source_revision;
use super::source_model::{
    replace_source_if_unchanged_locked, with_source_transaction, SourceWriteError,
};
use super::validation_json::{
    json_array_body, json_bool_field, json_object_bodies, json_str, json_string_array,
    json_string_field, json_usize_field, required_project_string, validate_ident_for_project,
};

pub(super) struct ProjectRenameSite {
    pub(super) rel: String,
    pub(super) kind: String,
    pub(super) title: String,
    pub(super) symbol: String,
    pub(super) module_path: String,
    pub(super) span: SourceSpan,
}

pub(super) struct ProjectRenamePlan {
    pub(super) changes: Vec<ProjectChange>,
    pub(super) sites: Vec<ProjectRenameSite>,
}

struct ProjectRenameSource {
    path: PathBuf,
    source_id: String,
    source: String,
    index: jet_semindex::SemIndex,
}

#[derive(Debug)]
pub(super) struct ProjectRenameError {
    pub(super) kind: &'static str,
    pub(super) message: String,
}

impl ProjectRenameError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

pub(super) fn required_project_touched_files(
    request: &str,
) -> Result<Vec<TouchedProjectFile>, String> {
    let files_body = json_array_body(request, "files")
        .ok_or_else(|| project_edit_error("bad_request", "missing `files`"))?;
    let mut files = Vec::new();
    for object in json_object_bodies(files_body) {
        let raw_path = json_string_field(object, "path")
            .ok_or_else(|| project_edit_error("bad_request", "touched file missing `path`"))?;
        let path = clean_project_rel_path(&raw_path)?;
        let revision = json_string_field(object, "revision")
            .ok_or_else(|| project_edit_error("bad_request", "touched file missing `revision`"))?;
        files.push(TouchedProjectFile { path, revision });
    }
    if files.is_empty() {
        return Err(project_edit_error(
            "bad_request",
            "Canvas project transactions must name touched files",
        ));
    }
    Ok(files)
}

pub(super) fn validate_touched_project_files(
    ctx: &ProjectContext,
    touched: &[TouchedProjectFile],
) -> Result<(), String> {
    for file in touched {
        if file.path.contains("..") || Path::new(&file.path).is_absolute() {
            return Err(project_edit_error(
                "bad_request",
                "Canvas project file paths must stay inside the project",
            ));
        }
        let Some(current) = ctx.files.iter().find(|f| f.path == file.path) else {
            if file.revision == "missing" {
                continue;
            }
            return Err(project_edit_error(
                "not_found",
                "Canvas project touched file is not in the projected source truth",
            ));
        };
        if current.revision != file.revision {
            return Err(project_edit_error(
                "conflict",
                "source file changed since this Canvas project was drawn",
            ));
        }
    }
    Ok(())
}

/// Build one semantic rename plan for both the read-only preview and the
/// project transaction. The selected definition anchor, not a matching name,
/// owns the edit set; this keeps same-spelled symbols in separate modules out
/// of one another's rename.
pub(super) fn prepare_project_rename(
    ctx: &ProjectContext,
    selected_path: &Path,
    from: &str,
    to: &str,
    touched: Option<&[TouchedProjectFile]>,
) -> Result<ProjectRenamePlan, ProjectRenameError> {
    if let Some(diagnostic) = ctx.authority_diagnostic.as_ref() {
        return Err(ProjectRenameError::new("stale", diagnostic.what.clone()));
    }
    let sources = project_rename_sources(ctx)?;
    let selected = sources
        .iter()
        .find(|source| source.path == selected_path)
        .ok_or_else(|| {
            ProjectRenameError::new(
                "not_found",
                "Canvas rename source is not in the projected project",
            )
        })?;
    let anchor = selected_rename_anchor(ctx, selected, from)?;
    let mut sites = Vec::new();
    let mut edits_by_source = BTreeMap::<String, Vec<(SourceSpan, String, String, String)>>::new();
    let mut seen = HashSet::new();

    for source in &sources {
        for definition in source.index.definitions() {
            if definition.name != from
                || !module_belongs_to(&ctx.project_root, &source.path, &definition.module_path)
                || !definition_matches_anchor(definition, &anchor)
            {
                continue;
            }
            add_project_rename_site(
                &mut sites,
                &mut edits_by_source,
                &mut seen,
                source,
                "definition",
                &definition.name,
                &definition.module_path,
                definition.def_span,
                to,
            );
        }
        for reference in source.index.references() {
            if reference.name != from
                || !module_belongs_to(&ctx.project_root, &source.path, &reference.module_path)
                || !reference_matches_anchor(reference, &anchor)
            {
                continue;
            }
            add_project_rename_site(
                &mut sites,
                &mut edits_by_source,
                &mut seen,
                source,
                "reference",
                &reference.name,
                &reference.module_path,
                reference.span,
                to,
            );
        }
    }

    if sites.is_empty() {
        return Err(ProjectRenameError::new(
            "not_found",
            "Canvas rename found no semantic definition or reference sites",
        ));
    }

    let mut changes = Vec::new();
    for (source_id, edits) in edits_by_source {
        let source = sources
            .iter()
            .find(|source| source.source_id == source_id)
            .expect("rename edit source came from project source set");
        let text_edits = edits
            .iter()
            .map(|(span, _, _, _)| edit(*span, to))
            .collect::<Vec<_>>();
        let after = FixEngine::apply_edits(&source.source, &text_edits).map_err(|_| {
            ProjectRenameError::new("overlap", "Canvas project rename edits overlapped")
        })?;
        changes.push(ProjectChange {
            path: source.path.clone(),
            rel: source.source_id.clone(),
            before: source.source.clone(),
            after,
            existed: true,
        });
    }
    changes.sort_by(|left, right| left.rel.cmp(&right.rel));

    if let Some(touched) = touched {
        for change in &changes {
            require_touched_revision(touched, &change.rel, &source_revision(&change.before))
                .map_err(|error| ProjectRenameError::new("bad_request", error))?;
        }
    }

    Ok(ProjectRenamePlan { changes, sites })
}

pub(super) fn apply_project_rename(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let selected_rel = json_string_field(request, "source_id")
        .map(|path| clean_project_rel_path(&path))
        .transpose()?
        .unwrap_or_else(|| rel_path(&ctx.project_root, &ctx.entry_path));
    if !ctx
        .files
        .iter()
        .any(|file| file.kind == "source" && file.path == selected_rel)
    {
        return Err(project_edit_error(
            "not_found",
            "Canvas rename source is not in the projected project",
        ));
    }
    let from = required_project_string(request, "from")?;
    let to = required_project_string(request, "to")?;
    validate_ident_for_project(&from)?;
    validate_ident_for_project(&to)?;
    let selected_path = ctx.project_root.join(&selected_rel);
    let plan = prepare_project_rename(ctx, &selected_path, &from, &to, Some(touched))
        .map_err(|error| project_edit_error(error.kind, &error.message))?;
    let op = json_string_field(request, "op").unwrap_or_else(|| "rename_binding".to_string());
    finish_project_changes(ctx, request, &op, plan.changes)
}

fn project_rename_sources(ctx: &ProjectContext) -> Result<Vec<ProjectRenameSource>, ProjectRenameError> {
    let mut sources = Vec::new();
    for file in ctx.files.iter().filter(|file| file.kind == "source") {
        let path = ctx.project_root.join(&file.path);
        if !ctx.parts.should_index(&path) {
            continue;
        }
        let source = fs::read_to_string(&path).map_err(|error| {
            ProjectRenameError::new(
                "stale",
                format!("Canvas project source moved or unreadable: {error}"),
            )
        })?;
        let revision = source_revision(&source);
        if revision != file.revision {
            return Err(ProjectRenameError::new(
                "conflict",
                "project source changed while Canvas was preparing its rename",
            ));
        }
        let index = jet_semindex::open(&path).map_err(|error| {
            ProjectRenameError::new("check", format!("Canvas rename index failed: {error}"))
        })?;
        sources.push(ProjectRenameSource {
            path,
            source_id: file.path.trim_start_matches("./").to_string(),
            source,
            index,
        });
    }
    Ok(sources)
}

fn selected_rename_anchor(
    ctx: &ProjectContext,
    selected: &ProjectRenameSource,
    from: &str,
) -> Result<DefinitionAnchor, ProjectRenameError> {
    let definitions = selected
        .index
        .definitions()
        .iter()
        .filter(|definition| {
            definition.name == from
                && module_belongs_to(&ctx.project_root, &selected.path, &definition.module_path)
        })
        .collect::<Vec<_>>();
    if definitions.len() == 1 {
        return Ok(DefinitionAnchor {
            module_path: definitions[0].module_path.clone(),
            kind: String::new(),
            def_span: definitions[0].def_span,
            semantic_identity: Some(definitions[0].identity.clone()),
        });
    }
    if definitions.len() > 1 {
        return Err(ProjectRenameError::new(
            "ambiguous",
            "Canvas rename needs one selected definition anchor",
        ));
    }

    let mut targets = selected
        .index
        .references()
        .iter()
        .filter(|reference| reference.name == from)
        .filter_map(|reference| reference.target.clone())
        .filter(|target| {
            ctx.files.iter().any(|file| {
                file.kind == "source"
                    && module_belongs_to(
                        &ctx.project_root,
                        &ctx.project_root.join(&file.path),
                        &target.module_path,
                    )
            })
        })
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    match targets.as_slice() {
        [target] => Ok(target.clone()),
        [] => Err(ProjectRenameError::new(
            "not_found",
            "Canvas rename found no selected semantic definition",
        )),
        _ => Err(ProjectRenameError::new(
            "ambiguous",
            "Canvas rename needs one selected semantic definition",
        )),
    }
}

fn add_project_rename_site(
    sites: &mut Vec<ProjectRenameSite>,
    edits_by_source: &mut BTreeMap<String, Vec<(SourceSpan, String, String, String)>>,
    seen: &mut HashSet<(String, usize, usize)>,
    source: &ProjectRenameSource,
    kind: &str,
    title: &str,
    module_path: &str,
    span: SourceSpan,
    to: &str,
) {
    if !seen.insert((source.source_id.clone(), span.start, span.end)) {
        return;
    }
    sites.push(ProjectRenameSite {
        rel: source.source_id.clone(),
        kind: kind.to_string(),
        title: title.to_string(),
        symbol: title.to_string(),
        module_path: module_path.to_string(),
        span,
    });
    edits_by_source.entry(source.source_id.clone()).or_default().push((
        span,
        kind.to_string(),
        title.to_string(),
        to.to_string(),
    ));
}

fn definition_matches_anchor(definition: &SymbolDef, anchor: &DefinitionAnchor) -> bool {
    definition.module_path == anchor.module_path
        && definition.def_span == anchor.def_span
        && anchor
            .semantic_identity
            .as_deref()
            .is_none_or(|identity| identity == definition.identity)
}

fn reference_matches_anchor(reference: &SymbolRef, anchor: &DefinitionAnchor) -> bool {
    let Some(target) = reference.target.as_ref() else {
        return false;
    };
    target.def_span == anchor.def_span
        && match (&target.semantic_identity, &anchor.semantic_identity) {
            (Some(left), Some(right)) => left == right,
            _ => target.module_path == anchor.module_path,
        }
}

pub(super) fn apply_project_add_dependency(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let manifest_rel = project_manifest_rel(ctx, request);
    if !touched.iter().any(|f| f.path == manifest_rel) {
        return Err(project_edit_error(
            "bad_request",
            "add_dependency must touch the edited Package file",
        ));
    }
    let name = required_project_string(request, "name")?;
    validate_ident_for_project(&name)?;
    let spec_text = required_project_string(request, "spec")?;
    let spec = project_dep_spec(&spec_text)?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before =
        fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    apply_canonical_dependency(
        ctx,
        request,
        &manifest_rel,
        before,
        &name,
        Some((spec_text, spec)),
    )
}

pub(super) fn apply_project_remove_dependency(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let manifest_rel = project_manifest_rel(ctx, request);
    if !touched.iter().any(|f| f.path == manifest_rel) {
        return Err(project_edit_error(
            "bad_request",
            "remove_dependency must touch the edited Package file",
        ));
    }
    let name = required_project_string(request, "name")?;
    validate_ident_for_project(&name)?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before =
        fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    apply_canonical_dependency(ctx, request, &manifest_rel, before, &name, None)
}

pub(super) fn apply_project_edit_pkg_field(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let manifest_rel = project_manifest_rel(ctx, request);
    if !touched.iter().any(|f| f.path == manifest_rel) {
        return Err(project_edit_error(
            "bad_request",
            "edit_pkg_field must touch the edited Package file",
        ));
    }
    let field = required_project_string(request, "field")?;
    let value = required_project_string(request, "value")?;
    validate_payload_field(&field, &value)?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before =
        fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    let after =
        replace_top_level_field(&before, &field, &format!("\"{}\"", manifest_string(&value)));
    jet_driver::Package::PackageFacts::parse(&after, manifest_rel.clone())
        .map_err(|error| project_edit_error("diagnostic", &error.to_string()))?;
    finish_project_changes(
        ctx,
        request,
        "edit_pkg_field",
        vec![ProjectChange {
            path: manifest_path,
            rel: manifest_rel,
            before,
            after,
            existed: true,
        }],
    )
}

pub(super) fn apply_project_add_target(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let manifest_rel = project_manifest_rel(ctx, request);
    if !touched.iter().any(|f| f.path == manifest_rel) {
        return Err(project_edit_error(
            "bad_request",
            "add_target must touch the edited Package file",
        ));
    }
    let name = required_project_string(request, "name")?;
    validate_ident_for_project(&name)?;
    let target = project_target_text(
        &json_string_field(request, "target").unwrap_or_else(|| "executable".to_string()),
    )?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before =
        fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    let facts = jet_driver::Package::PackageFacts::parse(&before, manifest_rel.clone())
        .map_err(|error| project_edit_error("diagnostic", &error.to_string()))?;
    if facts.outputs.contains_key(&name) {
        return Err(project_edit_error(
            "conflict",
            "the Package already declares this Output",
        ));
    }
    let kind = match target {
        "library" => "Library",
        "test" => "Check",
        "executable" | "example" => "Executable",
        _ => "Executable",
    };
    let entry = json_string_field(request, "entry").unwrap_or_else(|| "run".to_string());
    validate_ident_for_project(entry.rsplit('.').next().unwrap_or(&entry))?;
    let mut after = before.clone();
    if !after.ends_with('\n') {
        after.push('\n');
    }
    after.push_str(&format!(
        "{name}: Output :: .{kind}{{ name: \"{}\", entry: {entry} }}\n",
        manifest_string(&name)
    ));
    jet_driver::Package::PackageFacts::parse(&after, manifest_rel.clone())
        .map_err(|error| project_edit_error("diagnostic", &error.to_string()))?;
    finish_project_changes(
        ctx,
        request,
        "add_target",
        vec![ProjectChange {
            path: manifest_path,
            rel: manifest_rel,
            before,
            after,
            existed: true,
        }],
    )
}

fn project_manifest_rel(ctx: &ProjectContext, request: &str) -> String {
    json_string_field(request, "manifest").unwrap_or_else(|| {
        if let Some(root) = ctx.manifest_root.as_deref() {
            return rel_path(
                &ctx.project_root,
                &root.join(jet_driver::Syntax::PAYLOAD_FILE),
            );
        }
        if let Some(root) = ctx.ecosystem_root.as_deref() {
            return rel_path(
                &ctx.project_root,
                &root.join(jet_driver::Syntax::PACKAGE_FILE),
            );
        }
        jet_driver::Syntax::PAYLOAD_FILE.to_string()
    })
}

pub(super) fn apply_project_create_package(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let package_rel = clean_project_rel_path(
        &json_string_field(request, "package_path")
            .or_else(|| json_string_field(request, "new_package_path"))
            .ok_or_else(|| project_edit_error("bad_request", "missing `package_path`"))?,
    )?;
    let name = json_string_field(request, "name").unwrap_or_else(|| {
        Path::new(&package_rel)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("package")
            .to_string()
    });
    validate_ident_for_project(&name)?;
    let entry_rel = json_string_field(request, "entry")
        .map(|entry| clean_project_rel_path(&entry))
        .transpose()?
        .unwrap_or_else(|| format!("{package_rel}/{}", jet_driver::Syntax::DEFAULT_ENTRY_FILE));
    if !entry_rel.starts_with(&format!("{package_rel}/")) {
        return Err(project_edit_error(
            "bad_request",
            "create_package entry must live inside the package path",
        ));
    }
    if !entry_rel.ends_with(".jet") {
        return Err(project_edit_error(
            "bad_request",
            "create_package entry must be a .jet file",
        ));
    }
    let target = json_string_field(request, "target").unwrap_or_else(|| "executable".to_string());
    let target = project_target_text(&target)?;
    let manifest_rel = format!("{package_rel}/{}", jet_driver::Syntax::PACKAGE_FILE);
    require_touched_revision(touched, &manifest_rel, "missing")?;
    require_touched_revision(touched, &entry_rel, "missing")?;

    let manifest_path = ctx.project_root.join(&manifest_rel);
    let entry_path = ctx.project_root.join(&entry_rel);
    if manifest_path.exists() || entry_path.exists() {
        return Err(project_edit_error(
            "conflict",
            "create_package would overwrite existing source truth",
        ));
    }
    let output_kind = match target {
        "library" => "Library",
        "test" => "Check",
        "executable" | "example" => "Executable",
        _ => "Executable",
    };
    let manifest = format!(
        "name: \"{}\"\nversion: \"0.1.0\"\n{}: Output :: .{}{{ name: \"{}\", entry: run }}\n",
        name, name, output_kind, name
    );
    jet_driver::Package::PackageFacts::parse(&manifest, manifest_rel.clone())
        .map_err(|e| project_edit_error("diagnostic", &format!("{:?}", e)))?;
    let entry = if target == "library" {
        format!("pub fn {}_ready() Bool -> {{\n    return true\n}}\n", name)
    } else {
        format!("fn run() {{\n    print(\"{}\")\n}}\n", name)
    };
    let (tokens, lex_diags) = jet_driver::Lexer::lex(&entry);
    if let Some(d) = lex_diags
        .into_iter()
        .find(|d| d.severity == Severity::Error)
    {
        return Err(project_edit_error("diagnostic", &d.what));
    }
    jet_driver::Parser::parse(&tokens)
        .map(|_| ())
        .map_err(|mut diags| {
            let what = diags
                .pop()
                .map(|d| d.what)
                .unwrap_or_else(|| "created package entry did not parse".to_string());
            project_edit_error("diagnostic", &what)
        })?;
    let changes = vec![
        ProjectChange {
            path: manifest_path,
            rel: manifest_rel,
            before: String::new(),
            after: manifest,
            existed: false,
        },
        ProjectChange {
            path: entry_path,
            rel: entry_rel,
            before: String::new(),
            after: entry,
            existed: false,
        },
    ];
    finish_project_changes(ctx, request, "create_package", changes)
}

pub(super) fn apply_project_add_workspace_member(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let workspace_rel = json_string_field(request, "workspace")
        .map(|path| clean_project_rel_path(&path))
        .transpose()?
        .unwrap_or_else(|| jet_driver::Syntax::WORKSPACE_FILE.to_string());
    let member_path = clean_project_rel_path(&required_project_string(request, "member_path")?)?;
    let member_dir = ctx.project_root.join(&member_path);
    if !member_dir.join(jet_driver::Syntax::PACKAGE_FILE).is_file()
        && !member_dir.join(jet_driver::Syntax::PAYLOAD_FILE).is_file()
    {
        return Err(project_edit_error(
            "not_found",
            "workspace member must contain package.jet or pkg.jet",
        ));
    }
    let workspace_path = ctx.project_root.join(&workspace_rel);
    let before = fs::read_to_string(&workspace_path).unwrap_or_default();
    let existed = workspace_path.is_file();
    require_touched_revision(
        touched,
        &workspace_rel,
        if existed {
            ctx.files
                .iter()
                .find(|file| file.path == workspace_rel)
                .map(|file| file.revision.as_str())
                .unwrap_or("missing")
        } else {
            "missing"
        },
    )?;
    let after = if existed {
        add_workspace_member_to_source(&before, &member_path)?
    } else {
        format!(
            "module workspace {{\n    members: [\"./{}\"]\n}}\n",
            member_path
        )
    };
    jet_env_model::WorkspaceFile::evaluate(&after, &ctx.project_root)
        .map_err(|d| project_edit_error("diagnostic", &d.what))?;
    let change = ProjectChange {
        path: workspace_path,
        rel: workspace_rel,
        before,
        after,
        existed,
    };
    finish_project_changes(ctx, request, "add_workspace_member", vec![change])
}

pub(super) fn apply_project_add_env_service(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let env_rel = json_string_field(request, "env")
        .map(|path| clean_project_rel_path(&path))
        .transpose()?
        .unwrap_or_else(|| jet_driver::Syntax::ENV_FILE.to_string());
    let env_path = ctx.project_root.join(&env_rel);
    let existed = env_path.is_file();
    require_touched_revision(
        touched,
        &env_rel,
        if existed {
            ctx.files
                .iter()
                .find(|file| file.path == env_rel)
                .map(|file| file.revision.as_str())
                .unwrap_or("missing")
        } else {
            "missing"
        },
    )?;
    let name = required_project_string(request, "name")?;
    validate_ident_for_project(&name)?;
    let service = env_service_source(request, &name)?;
    let before = fs::read_to_string(&env_path).unwrap_or_default();
    let after = if existed {
        add_env_service_to_source(&before, &name, &service)?
    } else {
        format!("module env.dev {{\n    services: {{ {service} }}\n}}\n")
    };
    jet_env_model::ModuleEval::evaluate_env(&after, &ctx.project_root)
        .map_err(|d| project_edit_error("diagnostic", &d.what))?;
    finish_project_changes(
        ctx,
        request,
        "add_env_service",
        vec![ProjectChange {
            path: env_path,
            rel: env_rel,
            before,
            after,
            existed,
        }],
    )
}

fn env_service_source(request: &str, name: &str) -> Result<String, String> {
    let mut fields = vec![format!(
        "enable: {}",
        if json_bool_field(request, "enable").unwrap_or(true) {
            "true"
        } else {
            "false"
        }
    )];
    if let Some(port) = json_usize_field(request, "port") {
        fields.push(format!("ports: [{port}]"));
    }
    if request.contains("\"init\"") && json_string_array(request, "run").is_empty() {
        return Err(project_edit_error(
            "bad_request",
            "service start commands use typed `run: [program, args…]`, not `init`",
        ));
    }
    let run = json_string_array(request, "run");
    if !run.is_empty() {
        let rendered = run
            .iter()
            .map(|arg| format!("\"{}\"", manifest_string(arg)))
            .collect::<Vec<_>>()
            .join(", ");
        fields.push(format!("run: [{rendered}]"));
    }
    if let Some(ready) = json_string_field(request, "ready") {
        fields.push(format!("ready: \"{}\"", manifest_string(&ready)));
    }
    if let Some(shutdown) = json_string_field(request, "shutdown") {
        let policy =
            match shutdown.as_str() {
                "kill" | "Kill" => ".Kill",
                "term" | "Term" | "graceful" | "Graceful" => ".Term",
                _ => return Err(project_edit_error(
                    "bad_request",
                    "service shutdown must be `kill`, `term`, or `graceful`, not a shell command",
                )),
            };
        fields.push(format!("shutdown: {policy}"));
    }
    if let Some(data_dir) = json_string_field(request, "data_dir") {
        fields.push(format!("data_dir: \"{}\"", manifest_string(&data_dir)));
    }
    Ok(format!("{name}: {{ {} }}", fields.join(", ")))
}

fn add_env_service_to_source(src: &str, name: &str, service: &str) -> Result<String, String> {
    if src.contains(&format!("{name}:")) {
        return Ok(src.to_string());
    }
    if let Some((start, end)) = block_body_span(src, "services:") {
        let body = src[start..end].trim();
        let addition = if body.is_empty() {
            service.to_string()
        } else {
            format!(", {service}")
        };
        let mut out = String::with_capacity(src.len() + addition.len());
        out.push_str(&src[..end]);
        out.push_str(&addition);
        out.push_str(&src[end..]);
        return Ok(out);
    }
    let Some(close) = src.rfind('}') else {
        return Err(project_edit_error(
            "diagnostic",
            "env.jet module is missing its closing brace",
        ));
    };
    let insertion = format!("    services: {{ {service} }}\n");
    let mut out = String::with_capacity(src.len() + insertion.len());
    out.push_str(&src[..close]);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&insertion);
    out.push_str(&src[close..]);
    Ok(out)
}

fn add_workspace_member_to_source(src: &str, member_path: &str) -> Result<String, String> {
    let normalized = format!("./{}", member_path.trim_start_matches("./"));
    if src.contains(&format!("\"{}\"", normalized)) || src.contains(&format!("\"{}\"", member_path))
    {
        return Ok(src.to_string());
    }
    if workspace_find_covers(src, member_path) {
        return Ok(src.to_string());
    }
    let Some(members_pos) = src.find("members") else {
        return Err(project_edit_error(
            "unsupported",
            "workspace source has no members field Canvas can edit",
        ));
    };
    let Some(list_start_rel) = src[members_pos..].find('[') else {
        return Err(project_edit_error(
            "unsupported",
            "Canvas can add workspace members to explicit lists or covered find() dirs",
        ));
    };
    let list_start = members_pos + list_start_rel;
    let Some(list_end_rel) = src[list_start..].find(']') else {
        return Err(project_edit_error(
            "diagnostic",
            "workspace members list is missing its closing bracket",
        ));
    };
    let list_end = list_start + list_end_rel;
    let body = src[list_start + 1..list_end].trim();
    let addition = if body.is_empty() {
        format!("\"{}\"", normalized)
    } else {
        format!(", \"{}\"", normalized)
    };
    let mut out = String::with_capacity(src.len() + addition.len());
    out.push_str(&src[..list_end]);
    out.push_str(&addition);
    out.push_str(&src[list_end..]);
    Ok(out)
}

fn workspace_find_covers(src: &str, member_path: &str) -> bool {
    let Some(find_pos) = src.find("find(") else {
        return false;
    };
    let rest = &src[find_pos + "find(".len()..];
    let Some(start) = rest.find('"') else {
        return false;
    };
    let rest = &rest[start + 1..];
    let Some(end) = rest.find('"') else {
        return false;
    };
    let dir = rest[..end].trim_start_matches("./").trim_end_matches('/');
    member_path == dir || member_path.starts_with(&format!("{dir}/"))
}

fn validate_payload_field(field: &str, value: &str) -> Result<(), String> {
    match field {
        "name" => validate_ident_for_project(value),
        "version" | "jet" | "description" | "license" | "repository" | "edition" => Ok(()),
        _ => Err(project_edit_error(
            "bad_request",
            "Canvas can edit known payload string fields only",
        )),
    }
}

fn is_canonical_package_file(path: &str) -> bool {
    Path::new(path).file_name().and_then(|name| name.to_str())
        == Some(jet_driver::Syntax::PACKAGE_FILE)
}

/// #1664 criterion 2: one engine edits the `deps: { … }` block for both the
/// `jet add`/`jet remove` CLI verbs and this Canvas schema action —
/// `jet_driver::Manifest::add_dependency`/`remove_dependency` (comment- and
/// order-preserving). Canvas no longer hand-rolls a second BTreeMap-diff
/// rewrite of the whole block.
fn apply_canonical_dependency(
    ctx: &ProjectContext,
    request: &str,
    manifest_rel: &str,
    before: String,
    name: &str,
    add: Option<(String, jet_driver::Manifest::DepSpec)>,
) -> Result<String, String> {
    let adding = add.is_some();
    let after = match add {
        Some((spec_text, spec)) => {
            let facts = jet_driver::Package::PackageFacts::parse(&before, manifest_rel.to_string())
                .map_err(|error| project_edit_error("diagnostic", &error.to_string()))?;
            if let Some(existing) = facts.deps.get(name) {
                if jet_driver::Package::dep_display(existing) != spec_text {
                    return Err(project_edit_error(
                        "conflict",
                        "the canonical Package already declares this dependency with another spec",
                    ));
                }
            }
            jet_driver::Manifest::add_dependency(&before, name, &spec)
        }
        None => jet_driver::Manifest::remove_dependency(&before, name),
    };
    jet_driver::Package::PackageFacts::parse(&after, manifest_rel.to_string())
        .map_err(|error| project_edit_error("diagnostic", &error.to_string()))?;
    finish_project_changes(
        ctx,
        request,
        if adding {
            "add_dependency"
        } else {
            "remove_dependency"
        },
        vec![ProjectChange {
            path: ctx.project_root.join(manifest_rel),
            rel: manifest_rel.to_string(),
            before,
            after,
            existed: true,
        }],
    )
}

fn replace_top_level_field(src: &str, field: &str, replacement: &str) -> String {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    let mut entry_start = 0usize;
    let mut span = None;

    let mut inspect = |entry_start: usize, entry_end: usize| {
        if span.is_some() {
            return;
        }
        let entry = &src[entry_start..entry_end];
        let trimmed_start = entry.len() - entry.trim_start().len();
        let trimmed = entry.trim();
        let Some(colon) = trimmed.find(':') else {
            return;
        };
        if trimmed[..colon].trim() == field {
            let start = entry_start + trimmed_start + trimmed[..colon].find(field).unwrap_or(0);
            let end = entry_start + trimmed_start + trimmed.len();
            span = Some((start, end));
        }
    };

    for (index, byte) in src.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b',' | b'\n' if depth == 0 => {
                inspect(entry_start, index);
                entry_start = index + 1;
            }
            _ => {}
        }
    }
    inspect(entry_start, src.len());

    if let Some((start, end)) = span {
        let mut out = String::with_capacity(src.len() + replacement.len());
        out.push_str(&src[..start]);
        out.push_str(field);
        out.push_str(": ");
        out.push_str(replacement);
        out.push_str(&src[end..]);
        out
    } else {
        let mut out = src.to_string();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(field);
        out.push_str(": ");
        out.push_str(replacement);
        out.push('\n');
        out
    }
}

fn block_body_span(src: &str, label: &str) -> Option<(usize, usize)> {
    let label_pos = src.find(label)?;
    let open = label_pos + src[label_pos..].find('{')?;
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for i in open..bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open + 1, i));
                }
            }
            _ => {}
        }
    }
    None
}

fn manifest_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn require_touched_revision(
    touched: &[TouchedProjectFile],
    path: &str,
    revision: &str,
) -> Result<(), String> {
    if touched
        .iter()
        .any(|file| file.path == path && file.revision == revision)
    {
        return Ok(());
    }
    Err(project_edit_error(
        "bad_request",
        "project transaction touched files do not match the operation",
    ))
}

pub(super) fn clean_project_rel_path(path: &str) -> Result<String, String> {
    let path = path.trim().trim_start_matches("./");
    if path.is_empty() || path.contains('\\') || Path::new(path).is_absolute() {
        return Err(project_edit_error(
            "bad_request",
            "Canvas project paths must be relative source paths",
        ));
    }
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::Normal(part) => {
                let Some(part) = part.to_str() else {
                    return Err(project_edit_error(
                        "bad_request",
                        "Canvas project paths must be UTF-8",
                    ));
                };
                if is_reserved_project_path_part(part) {
                    return Err(project_edit_error(
                        "bad_request",
                        "Canvas project paths cannot target reserved project directories",
                    ));
                }
                parts.push(part);
            }
            _ => {
                return Err(project_edit_error(
                    "bad_request",
                    "Canvas project paths must stay inside the project",
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn is_reserved_project_path_part(part: &str) -> bool {
    matches!(part, ".git" | ".jet" | "target" | "build")
}

fn project_target_text(target: &str) -> Result<&'static str, String> {
    match target {
        "library" => Ok("library"),
        "executable" => Ok("executable"),
        "test" => Ok("test"),
        "example" => Ok("example"),
        _ => Err(project_edit_error(
            "bad_request",
            "unknown Canvas package target",
        )),
    }
}

fn project_dep_spec(spec: &str) -> Result<jet_driver::Manifest::DepSpec, String> {
    if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') {
        return Ok(jet_driver::Manifest::DepSpec::Path {
            path: spec.to_string(),
        });
    }
    if spec.starts_with("git@") {
        return Err(project_edit_error(
            "unsupported",
            "Canvas project transactions need an explicit version or path dependency here",
        ));
    }
    if spec.trim().is_empty() {
        return Err(project_edit_error(
            "bad_request",
            "dependency spec is empty",
        ));
    }
    Ok(jet_driver::Manifest::DepSpec::Registry(spec.to_string()))
}

fn finish_project_changes(
    ctx: &ProjectContext,
    request: &str,
    op: &str,
    mut changes: Vec<ProjectChange>,
) -> Result<String, String> {
    if matches!(op, "rename_binding" | "rename_function") {
        normalize_and_format_project_changes(ctx, &mut changes)?;
    } else {
        normalize_and_validate_project_changes(ctx, &mut changes)?;
    }
    if matches!(op, "rename_binding" | "rename_function") {
        validate_project_rename_overlay(ctx, &changes)?;
    }
    let preview =
        json_bool_field(request, "preview").unwrap_or(false) || op.starts_with("preview_");
    let changed = changes.iter().any(|c| c.before != c.after);
    let diff = changes
        .iter()
        .map(|c| format!("diff -- {}\n{}", c.rel, simple_diff(&c.before, &c.after)))
        .collect::<Vec<_>>()
        .join("\n");
    if !preview {
        write_project_changes_with_rollback(ctx, &changes)?;
    }
    let touched_files = changes
        .iter()
        .map(|c| {
            let after_revision = source_revision(&c.after);
            format!(
                "{{\"path\":{},\"revision\":{},\"changed\":{}}}",
                json_str(&c.rel),
                json_str(&after_revision),
                if c.before != c.after { "true" } else { "false" }
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let next_project_revision = if preview {
        project_revision_after_changes(ctx, &changes)
    } else {
        project_context_for_entry(&ctx.entry_path).project_revision
    };
    Ok(project_edit_ok(
        op,
        preview,
        changed,
        &ctx.project_revision,
        &next_project_revision,
        &touched_files,
        &diff,
    ))
}

pub(super) fn normalize_and_validate_project_changes(
    ctx: &ProjectContext,
    changes: &mut [ProjectChange],
) -> Result<(), String> {
    normalize_and_validate_project_changes_inner(ctx, changes, true)
}

pub(super) fn normalize_and_format_project_changes(
    ctx: &ProjectContext,
    changes: &mut [ProjectChange],
) -> Result<(), String> {
    normalize_and_validate_project_changes_inner(ctx, changes, false)
}

fn normalize_and_validate_project_changes_inner(
    ctx: &ProjectContext,
    changes: &mut [ProjectChange],
    check_source: bool,
) -> Result<(), String> {
    for change in changes {
        if change.before == change.after {
            continue;
        }
        if is_canonical_package_file(&change.rel)
            || change
                .rel
                .ends_with(&format!("/{}", jet_driver::Syntax::PAYLOAD_FILE))
            || change.rel == jet_driver::Syntax::PAYLOAD_FILE
        {
            jet_driver::Package::PackageFacts::parse(&change.after, change.rel.clone())
                .map_err(|error| project_edit_error("diagnostic", &error.to_string()))?;
        } else if change
            .rel
            .ends_with(&format!("/{}", jet_driver::Syntax::WORKSPACE_FILE))
            || change.rel == jet_driver::Syntax::WORKSPACE_FILE
        {
            jet_env_model::WorkspaceFile::evaluate(&change.after, &ctx.project_root)
                .map_err(|d| project_edit_error("diagnostic", &d.what))?;
        } else if change
            .rel
            .ends_with(&format!("/{}", jet_driver::Syntax::ENV_FILE))
            || change.rel == jet_driver::Syntax::ENV_FILE
        {
            jet_env_model::ModuleEval::evaluate_env(&change.after, &ctx.project_root)
                .map_err(|d| project_edit_error("diagnostic", &d.what))?;
        } else if change.path.extension().and_then(|e| e.to_str())
            == Some(jet_driver::Syntax::FILE_EXT)
        {
            change.after =
                jet_driver::Formatter::format_source(&change.after).map_err(|diags| {
                    project_edit_error(
                        "diagnostic",
                        &jet_driver::Diagnostics::render_all(
                            &change.path.display().to_string(),
                            &change.after,
                            &diags,
                        ),
                    )
                })?;
            if check_source {
                validate_project_jet_overlay(&change.path, &change.after)?;
            }
        } else {
            return Err(project_edit_error(
                "bad_request",
                "Canvas project transactions may only write Jet source truth",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_project_rename_overlay(
    ctx: &ProjectContext,
    changes: &[ProjectChange],
) -> Result<(), String> {
    let overlays = changes
        .iter()
        .map(|change| (change.path.as_path(), change.after.as_str()))
        .collect::<Vec<_>>();
    jet_semindex::open_with_overlays(&ctx.entry_path, &overlays)
        .map(|_| ())
        .map_err(|error| project_edit_error("diagnostic", &error.to_string()))
}

fn validate_project_jet_overlay(path: &Path, src: &str) -> Result<(), String> {
    let shown = path.display().to_string();
    let diags = if path.exists() {
        let (diags, _) = jet_driver::Driver::check_file(&shown, Some((path, src)), false);
        diags
    } else {
        jet_driver::Driver::check_eval(src, &shown)
    };
    let errors = diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect::<Vec<_>>();
    if errors.is_empty() {
        return Ok(());
    }
    Err(project_edit_error(
        "diagnostic",
        &jet_driver::Diagnostics::render_all(&shown, src, &errors),
    ))
}

fn write_project_changes_with_rollback(
    ctx: &ProjectContext,
    changes: &[ProjectChange],
) -> Result<(), String> {
    with_source_transaction(|| {
        let current = project_context_for_entry(&ctx.entry_path);
        if current.authority_diagnostic.is_some()
            || current.project_revision != ctx.project_revision
        {
            return Err(SourceWriteError::Conflict);
        }
        let mut written = Vec::new();
        for change in changes {
            if change.before == change.after {
                continue;
            }
            if let Some(parent) = change.path.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    rollback_project_writes(&written);
                    return Err(SourceWriteError::Io(error));
                }
            }
            let expected = change.existed.then_some(change.before.as_str());
            if let Err(error) = replace_source_if_unchanged_locked(
                &change.path,
                expected,
                Some(change.after.as_str()),
            ) {
                rollback_project_writes(&written);
                return Err(error);
            }
            written.push(ProjectWriteBackup {
                path: change.path.clone(),
                before: change.before.clone(),
                after: change.after.clone(),
                existed: change.existed,
            });
        }
        Ok(())
    })
    .map_err(|error| match error {
        SourceWriteError::Conflict => project_edit_error(
            "conflict",
            "source changed while this Canvas project transaction was prepared",
        ),
        SourceWriteError::Io(error) => project_edit_error("io", &error.to_string()),
    })
}

pub(super) fn project_revision_after_changes(
    ctx: &ProjectContext,
    changes: &[ProjectChange],
) -> String {
    let mut files = ctx.files.clone();
    for change in changes {
        if let Some(file) = files.iter_mut().find(|file| file.path == change.rel) {
            file.revision = source_revision(&change.after);
        } else {
            files.push(ProjectFileRec {
                path: change.rel.clone(),
                revision: source_revision(&change.after),
                kind: project_file_kind_for_rel(&change.rel).to_string(),
            });
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    project_revision_from_files(&files)
}

fn rollback_project_writes(written: &[ProjectWriteBackup]) {
    for backup in written.iter().rev() {
        let expected = Some(backup.after.as_str());
        let candidate = backup.existed.then_some(backup.before.as_str());
        let _ = replace_source_if_unchanged_locked(&backup.path, expected, candidate);
    }
}

struct ProjectWriteBackup {
    path: PathBuf,
    before: String,
    after: String,
    existed: bool,
}

fn project_file_kind_for_rel(rel: &str) -> &'static str {
    if is_canonical_package_file(rel) {
        "package"
    } else if rel.ends_with(&format!("/{}", jet_driver::Syntax::PAYLOAD_FILE))
        || rel == jet_driver::Syntax::PAYLOAD_FILE
    {
        "manifest"
    } else if rel.ends_with(&format!("/{}", jet_driver::Syntax::WORKSPACE_FILE))
        || rel == jet_driver::Syntax::WORKSPACE_FILE
    {
        "workspace"
    } else if rel.ends_with(&format!("/{}", jet_driver::Syntax::ENV_FILE))
        || rel == jet_driver::Syntax::ENV_FILE
    {
        "env"
    } else if rel == jet_driver::Syntax::UNIFIED_LOCK_FILE {
        "lock"
    } else {
        "source"
    }
}

pub(super) fn diagnostic_json(d: &Diagnostic) -> String {
    format!(
        "{{\"code\":{},\"what\":{},\"why\":{},\"fix\":{}}}",
        json_str(&d.code),
        json_str(&d.what),
        json_str(&d.why),
        json_str(&d.fix)
    )
}

pub(super) fn module_belongs_to(project_root: &Path, source_path: &Path, module_path: &str) -> bool {
    let source_id = rel_path(project_root, source_path);
    let source_id = source_id.trim_start_matches("./");
    let module_path = module_path.replace('\\', "/");
    module_path == source_id
        || module_path == source_path.to_string_lossy().replace('\\', "/")
        || module_path.ends_with(&format!("/{source_id}"))
}

pub(super) fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
