use std::fs;
use std::path::{Path, PathBuf};

use jet_driver::Diagnostics::{Diagnostic, Severity};

use super::graph_helpers::{project_edit_error, project_edit_ok, simple_diff};
use super::project_scan::{
    ProjectChange, ProjectContext, ProjectFileRec, TouchedProjectFile, project_context_for_entry,
    project_revision_from_files,
};
use super::schema_api::source_revision;
use super::validation_json::{
    json_array_body, json_bool_field, json_object_bodies, json_str, json_string_field,
    json_usize_field, required_project_string, validate_ident_for_project,
};

pub(super) fn required_project_touched_files(request: &str) -> Result<Vec<TouchedProjectFile>, String> {
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

pub(super) fn apply_project_add_dependency(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let manifest_rel = json_string_field(request, "manifest").unwrap_or_else(|| {
        ctx.manifest_root
            .as_deref()
            .map(|root| rel_path(&ctx.project_root, &root.join(jet_driver::Syntax::PAYLOAD_FILE)))
            .unwrap_or_else(|| jet_driver::Syntax::PAYLOAD_FILE.to_string())
    });
    if !touched.iter().any(|f| f.path == manifest_rel) {
        return Err(project_edit_error(
            "bad_request",
            "add_dependency must touch the edited pkg.jet",
        ));
    }
    let name = required_project_string(request, "name")?;
    validate_ident_for_project(&name)?;
    let spec_text = required_project_string(request, "spec")?;
    let spec = project_dep_spec(&spec_text)?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before = fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    let after = jet_driver::Manifest::add_dependency(&before, &name, &spec);
    jet_driver::PackageManifest::parse(&after)
        .map_err(|e| project_edit_error("diagnostic", &format!("{:?}", e)))?;
    let change = ProjectChange {
        path: manifest_path,
        rel: manifest_rel,
        before,
        after,
    };
    finish_project_changes(ctx, request, "add_dependency", vec![change])
}

pub(super) fn apply_project_remove_dependency(
    ctx: &ProjectContext,
    request: &str,
    touched: &[TouchedProjectFile],
) -> Result<String, String> {
    let manifest_rel = json_string_field(request, "manifest").unwrap_or_else(|| {
        ctx.manifest_root
            .as_deref()
            .map(|root| rel_path(&ctx.project_root, &root.join(jet_driver::Syntax::PAYLOAD_FILE)))
            .unwrap_or_else(|| jet_driver::Syntax::PAYLOAD_FILE.to_string())
    });
    if !touched.iter().any(|f| f.path == manifest_rel) {
        return Err(project_edit_error(
            "bad_request",
            "remove_dependency must touch the edited pkg.jet",
        ));
    }
    let name = required_project_string(request, "name")?;
    validate_ident_for_project(&name)?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before = fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    let after = jet_driver::Manifest::remove_dependency(&before, &name);
    jet_driver::PackageManifest::parse(&after)
        .map_err(|e| project_edit_error("diagnostic", &format!("{:?}", e)))?;
    let change = ProjectChange {
        path: manifest_path,
        rel: manifest_rel,
        before,
        after,
    };
    finish_project_changes(ctx, request, "remove_dependency", vec![change])
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
            "edit_pkg_field must touch the edited pkg.jet",
        ));
    }
    let field = required_project_string(request, "field")?;
    let value = required_project_string(request, "value")?;
    validate_payload_field(&field, &value)?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before = fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    let after = set_manifest_payload_field(&before, &field, &value)?;
    jet_driver::PackageManifest::parse(&after)
        .map_err(|e| project_edit_error("diagnostic", &format!("{:?}", e)))?;
    finish_project_changes(
        ctx,
        request,
        "edit_pkg_field",
        vec![ProjectChange {
            path: manifest_path,
            rel: manifest_rel,
            before,
            after,
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
            "add_target must touch the edited pkg.jet",
        ));
    }
    let name = required_project_string(request, "name")?;
    validate_ident_for_project(&name)?;
    let target = project_target_text(
        &json_string_field(request, "target").unwrap_or_else(|| "executable".to_string()),
    )?;
    let manifest_path = ctx.project_root.join(&manifest_rel);
    let before = fs::read_to_string(&manifest_path).map_err(|e| project_edit_error("io", &e.to_string()))?;
    let after = add_manifest_target(&before, &name, target)?;
    jet_driver::PackageManifest::parse(&after)
        .map_err(|e| project_edit_error("diagnostic", &format!("{:?}", e)))?;
    finish_project_changes(
        ctx,
        request,
        "add_target",
        vec![ProjectChange {
            path: manifest_path,
            rel: manifest_rel,
            before,
            after,
        }],
    )
}

fn project_manifest_rel(ctx: &ProjectContext, request: &str) -> String {
    json_string_field(request, "manifest").unwrap_or_else(|| {
        ctx.manifest_root
            .as_deref()
            .map(|root| rel_path(&ctx.project_root, &root.join(jet_driver::Syntax::PAYLOAD_FILE)))
            .unwrap_or_else(|| jet_driver::Syntax::PAYLOAD_FILE.to_string())
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
        .unwrap_or_else(|| format!("{package_rel}/main.jet"));
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
    let manifest_rel = format!("{package_rel}/{}", jet_driver::Syntax::PAYLOAD_FILE);
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
    let manifest = format!(
        "payload: {{\n    name: \"{}\",\n    version: \"0.1.0\",\n}}\npackages: {{\n    {}: {},\n}}\n",
        name, name, target
    );
    jet_driver::PackageManifest::parse(&manifest)
        .map_err(|e| project_edit_error("diagnostic", &format!("{:?}", e)))?;
    let entry = if target == "library" {
        format!("pub fn {}_ready() -> Bool {{\n    return true\n}}\n", name)
    } else {
        format!("fn run() {{\n    print(\"{}\")\n}}\n", name)
    };
    let (tokens, lex_diags) = jet_driver::Lexer::lex(&entry);
    if let Some(d) = lex_diags.into_iter().find(|d| d.severity == Severity::Error) {
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
        },
        ProjectChange {
            path: entry_path,
            rel: entry_rel,
            before: String::new(),
            after: entry,
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
    if !member_dir.join(jet_driver::Syntax::PAYLOAD_FILE).is_file() {
        return Err(project_edit_error(
            "not_found",
            "workspace member must contain a pkg.jet",
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
    jetpack::WorkspaceFile::evaluate(&after, &ctx.project_root)
        .map_err(|d| project_edit_error("diagnostic", &d.what))?;
    let change = ProjectChange {
        path: workspace_path,
        rel: workspace_rel,
        before,
        after,
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
    if let Some(init) = json_string_field(request, "init") {
        fields.push(format!("init: \"{}\"", manifest_string(&init)));
    }
    if let Some(ready) = json_string_field(request, "ready") {
        fields.push(format!("ready: \"{}\"", manifest_string(&ready)));
    }
    if let Some(shutdown) = json_string_field(request, "shutdown") {
        fields.push(format!("shutdown: \"{}\"", manifest_string(&shutdown)));
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
    if src.contains(&format!("\"{}\"", normalized)) || src.contains(&format!("\"{}\"", member_path)) {
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

fn set_manifest_payload_field(src: &str, field: &str, value: &str) -> Result<String, String> {
    let (start, end) = block_body_span(src, "payload:")
        .ok_or_else(|| project_edit_error("diagnostic", "pkg.jet has no payload block"))?;
    set_block_field(src, start, end, field, &format!("\"{}\"", manifest_string(value)))
}

fn add_manifest_target(src: &str, name: &str, target: &str) -> Result<String, String> {
    if let Some((start, end)) = block_body_span(src, "packages:") {
        return set_block_field(src, start, end, name, target);
    }
    let mut out = src.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("packages: {{\n    {name}: {target},\n}}\n"));
    Ok(out)
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

fn set_block_field(
    src: &str,
    start: usize,
    end: usize,
    field: &str,
    value: &str,
) -> Result<String, String> {
    validate_ident_for_project(field)?;
    let body = &src[start..end];
    let mut offset = start;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&format!("{field}:")) {
            let indent_len = line.len() - trimmed.len();
            let indent = &line[..indent_len];
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            let replacement = format!("{indent}{field}: {value},{newline}");
            let mut out = String::with_capacity(src.len() + replacement.len());
            out.push_str(&src[..offset]);
            out.push_str(&replacement);
            out.push_str(&src[offset + line.len()..]);
            return Ok(out);
        }
        offset += line.len();
    }
    let insertion = if body.trim().is_empty() {
        format!("\n    {field}: {value},")
    } else {
        format!("    {field}: {value},\n")
    };
    let mut out = String::with_capacity(src.len() + insertion.len());
    out.push_str(&src[..end]);
    out.push_str(&insertion);
    out.push_str(&src[end..]);
    Ok(out)
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
        "benchmark" => Ok("benchmark"),
        _ => Err(project_edit_error(
            "bad_request",
            "unknown Canvas package target",
        )),
    }
}

fn project_dep_spec(spec: &str) -> Result<jet_driver::Manifest::DepSpec, String> {
    if let Some(path) = spec.strip_prefix("path@") {
        if path.trim().is_empty() {
            return Err(project_edit_error("bad_request", "path dependency needs a path"));
        }
        return Ok(jet_driver::Manifest::DepSpec::Path {
            path: path.to_string(),
        });
    }
    if spec.starts_with("git@") {
        return Err(project_edit_error(
            "unsupported",
            "Canvas project transactions need an explicit version or path dependency here",
        ));
    }
    if spec.trim().is_empty() {
        return Err(project_edit_error("bad_request", "dependency spec is empty"));
    }
    Ok(jet_driver::Manifest::DepSpec::Registry(spec.to_string()))
}

fn finish_project_changes(
    ctx: &ProjectContext,
    request: &str,
    op: &str,
    mut changes: Vec<ProjectChange>,
) -> Result<String, String> {
    normalize_and_validate_project_changes(ctx, &mut changes)?;
    let preview = json_bool_field(request, "preview").unwrap_or(false)
        || op.starts_with("preview_");
    let changed = changes.iter().any(|c| c.before != c.after);
    let diff = changes
        .iter()
        .map(|c| {
            format!(
                "diff -- {}\n{}",
                c.rel,
                simple_diff(&c.before, &c.after)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !preview {
        write_project_changes_with_rollback(&changes)?;
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
        let mut files = ctx.files.clone();
        for change in &changes {
            if let Some(file) = files.iter_mut().find(|f| f.path == change.rel) {
                file.revision = source_revision(&change.after);
            } else {
                files.push(ProjectFileRec {
                    path: change.rel.clone(),
                    revision: source_revision(&change.after),
                    kind: project_file_kind_for_rel(&change.rel).to_string(),
                });
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        project_revision_from_files(&files)
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

fn normalize_and_validate_project_changes(
    ctx: &ProjectContext,
    changes: &mut [ProjectChange],
) -> Result<(), String> {
    for change in changes {
        if change.before == change.after {
            continue;
        }
        if change.rel.ends_with(&format!("/{}", jet_driver::Syntax::PAYLOAD_FILE))
            || change.rel == jet_driver::Syntax::PAYLOAD_FILE
        {
            jet_driver::PackageManifest::parse(&change.after)
                .map_err(|e| project_edit_error("diagnostic", &format!("{:?}", e)))?;
        } else if change.rel.ends_with(&format!("/{}", jet_driver::Syntax::WORKSPACE_FILE))
            || change.rel == jet_driver::Syntax::WORKSPACE_FILE
        {
            jetpack::WorkspaceFile::evaluate(&change.after, &ctx.project_root)
                .map_err(|d| project_edit_error("diagnostic", &d.what))?;
        } else if change.rel.ends_with(&format!("/{}", jet_driver::Syntax::ENV_FILE))
            || change.rel == jet_driver::Syntax::ENV_FILE
        {
            jet_env_model::ModuleEval::evaluate_env(&change.after, &ctx.project_root)
                .map_err(|d| project_edit_error("diagnostic", &d.what))?;
        } else if change
            .path
            .extension()
            .and_then(|e| e.to_str())
            == Some(jet_driver::Syntax::FILE_EXT)
        {
            change.after = jet_driver::Formatter::format_source(&change.after).map_err(|diags| {
                project_edit_error(
                    "diagnostic",
                    &jet_driver::Diagnostics::render_all(
                        &change.path.display().to_string(),
                        &change.after,
                        &diags,
                    ),
                )
            })?;
            validate_project_jet_overlay(&change.path, &change.after)?;
        } else {
            return Err(project_edit_error(
                "bad_request",
                "Canvas project transactions may only write Jet source truth",
            ));
        }
    }
    Ok(())
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

fn write_project_changes_with_rollback(changes: &[ProjectChange]) -> Result<(), String> {
    let mut written = Vec::new();
    for change in changes {
        if change.before == change.after {
            continue;
        }
        let existed = change.path.exists();
        if let Some(parent) = change.path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                rollback_project_writes(&written);
                return Err(project_edit_error("io", &e.to_string()));
            }
        }
        if let Err(e) = fs::write(&change.path, &change.after) {
            rollback_project_writes(&written);
            return Err(project_edit_error("io", &e.to_string()));
        }
        written.push(ProjectWriteBackup {
            path: change.path.clone(),
            before: change.before.clone(),
            existed,
        });
    }
    Ok(())
}

fn rollback_project_writes(written: &[ProjectWriteBackup]) {
    for backup in written.iter().rev() {
        if backup.existed {
            let _ = fs::write(&backup.path, &backup.before);
        } else {
            let _ = fs::remove_file(&backup.path);
        }
    }
}

struct ProjectWriteBackup {
    path: PathBuf,
    before: String,
    existed: bool,
}

fn project_file_kind_for_rel(rel: &str) -> &'static str {
    if rel.ends_with(&format!("/{}", jet_driver::Syntax::PAYLOAD_FILE))
        || rel == jet_driver::Syntax::PAYLOAD_FILE
    {
        "manifest"
    } else if rel.ends_with(&format!("/{}", jet_driver::Syntax::WORKSPACE_FILE))
        || rel == jet_driver::Syntax::WORKSPACE_FILE
    {
        "workspace"
    } else if rel.ends_with(&format!("/{}", jet_driver::Syntax::ENV_FILE)) || rel == jet_driver::Syntax::ENV_FILE {
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

pub(super) fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
