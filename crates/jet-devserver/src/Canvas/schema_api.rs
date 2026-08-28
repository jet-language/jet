use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jet_driver::Diagnostics::{Diagnostic, ReportEnvelope, Severity};
use jet_driver::SHA256;
use jet_semindex::{semantic_ops_for_file, SemanticOp, SourceSpan};

use super::debug_source_git::{
    canonical_path, debug_diagnostics_error, debug_error, debug_ok, debug_stop_ok, git_output,
    git_relative_path, git_root, line_from_anchor, required_debug_string, untracked_diff,
    validate_debug_breakpoint_anchors, DebugSessions, DebugTier,
};
use super::graph_helpers::{
    diagnostics_json, edit_error, project_edit_conflict, project_edit_error, query_error,
};
use super::project_scan::{
    env_project_json, lock_project_json, packages_project_json, project_context_for_entry,
    project_file, project_file_with_runtime, targets_project_json, workspace_project_json,
    EnvProjectJson, ProjectContext,
};
use super::project_transactions::{
    apply_project_add_dependency, apply_project_add_env_service, apply_project_add_target,
    apply_project_add_workspace_member, apply_project_create_package, apply_project_edit_pkg_field,
    apply_project_remove_dependency, apply_project_rename, clean_project_rel_path, diagnostic_json,
    rel_path, required_project_touched_files, validate_touched_project_files,
};
use super::query_actions::{
    canvas_actions, canvas_core_catalog, canvas_core_catalog_query, canvas_find,
    canvas_preview_rename, canvas_project_find, canvas_project_preview_rename,
    canvas_project_references, canvas_references, canvas_source_to_graph,
};
use super::validation_json::{
    json_bool_field, json_str, json_string_array, json_string_field, json_usize_array,
    json_usize_field, required_project_string, required_query_string, required_string,
    validate_query_ident,
};
use super::edit_transaction;
use super::source_model::source_revision;

pub const GRAPH_SCHEMA_VERSION: u32 = 1;
pub use edit_transaction::EDIT_SCHEMA_VERSION;
pub const DEBUG_SCHEMA_VERSION: u32 = 1;
pub const QUERY_SCHEMA_VERSION: u32 = 1;
pub const ACTION_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const CORE_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const PROOF_SCHEMA_VERSION: u32 = 1;

/// Put the Canvas command payload behind the one machine-output envelope.
///
/// The existing protocol object remains named command data rather than a
/// second machine-output door.
fn canvas_machine_output(action: &str, status: &str, ok: bool, payload: &str) -> String {
    ReportEnvelope::status_record("tool", status, ok, action)
        .with_json_field("canvas", payload.trim())
        .json()
}

fn canvas_machine_success(action: &str, payload: &str) -> String {
    canvas_machine_output(action, "ok", true, payload)
}

fn canvas_machine_error(action: &str, payload: &str) -> String {
    canvas_machine_output(action, "error", false, payload)
}

/// Project a checked Jet file into the public Canvas graph schema.
pub fn graph_json_for_file(path: &Path) -> Result<String, Vec<Diagnostic>> {
    project_file(path).map(|p| canvas_machine_success("canvas.graph", &p.json))
}

/// Render graph projection diagnostics through the same machine-output door.
pub fn graph_json_error_for_file(path: &Path, diags: &[Diagnostic]) -> String {
    let src = fs::read_to_string(path).unwrap_or_default();
    canvas_machine_error(
        "canvas.graph",
        &query_error(
            "diagnostic",
            &jet_driver::Diagnostics::render_all(&path.display().to_string(), &src, diags),
        ),
    )
}

/// Project a file graph selected from the entry file's package/workspace graph.
pub fn graph_json_for_entry_source(
    entry: &Path,
    source_id: Option<&str>,
) -> Result<String, String> {
    let path = resolve_entry_source_path(entry, source_id)
        .map_err(|error| canvas_machine_error("canvas.graph", &error))?;
    graph_json_for_file(&path).map_err(|diags| graph_json_error_for_file(&path, &diags))
}

/// Project executed Event/AsyncEvent/DecisionHook facts from the existing
/// owner-checked, payload-free live snapshot onto a Canvas graph.
pub fn graph_json_for_entry_source_with_live_pid(
    entry: &Path,
    source_id: Option<&str>,
    pid: u32,
) -> Result<String, String> {
    let snapshot = crate::LiveInspect::read(pid)
        .map_err(|message| canvas_machine_error("canvas.graph", &query_error("live", &message)))?;
    let runtime_events = runtime_events_json(&snapshot)
        .map_err(|error| canvas_machine_error("canvas.graph", &error))?;
    let path = resolve_entry_source_path(entry, source_id)
        .map_err(|error| canvas_machine_error("canvas.graph", &error))?;
    project_file_with_runtime(&path, Some(&runtime_events))
        .map(|projection| canvas_machine_success("canvas.graph", &projection.json))
        .map_err(|diags| graph_json_error_for_file(&path, &diags))
}

fn runtime_events_json(snapshot: &str) -> Result<String, String> {
    let rendered = jet_debug::render_event_observations(snapshot)
        .map_err(|message| query_error("live", &message))?;
    let events = rendered
        .lines()
        .map(|line| {
            let fields = line
                .split_whitespace()
                .filter_map(|field| field.split_once('='))
                .collect::<HashMap<_, _>>();
            let number = |key| {
                fields
                    .get(key)
                    .copied()
                    .ok_or_else(|| query_error("live", "runtime event observation is incomplete"))
            };
            let text = |key| number(key).map(json_str);
            Ok(format!(
                "{{\"sequence\":{},\"source\":{},\"event_id\":{},\"owner_id\":{},\"subscription_id\":{},\"dispatch_id\":{},\"lifecycle\":{},\"queued\":{},\"blocked\":{},\"running\":{},\"capacity\":{},\"overflow\":{},\"priority\":{},\"failure\":{},\"terminal\":{}}}",
                number("sequence")?,
                text("source")?,
                number("event")?,
                number("owner")?,
                number("subscription")?,
                number("dispatch")?,
                text("lifecycle")?,
                number("queued")?,
                number("blocked")?,
                number("running")?,
                number("capacity")?,
                text("overflow")?,
                number("priority")?,
                text("failure")?,
                text("terminal")?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "{{\"source_truth\":\"executed_runtime_observations\",\"events\":[{}]}}",
        events.join(",")
    ))
}

/// Resolve a project-relative source id without escaping the projected source truth.
pub fn project_path_for_source_id(entry: &Path, source_id: &str) -> Option<PathBuf> {
    // Project-part discovery parses every source file. Keep that recursive
    // parser work on the canonical compiler stack even before graph
    // projection crosses the same boundary.
    jet_driver::run_compiler_work(|| project_path_for_source_id_on_compiler_stack(entry, source_id))
}

fn project_path_for_source_id_on_compiler_stack(entry: &Path, source_id: &str) -> Option<PathBuf> {
    let wanted = clean_project_rel_path(source_id).ok()?;
    if Path::new(&wanted).extension().and_then(|e| e.to_str()) != Some(jet_driver::Syntax::FILE_EXT)
    {
        return None;
    }
    let ctx = project_context_for_entry(entry);
    if let Some(file) = ctx
        .files
        .iter()
        .find(|f| f.kind == "source" && f.path.trim_start_matches("./") == wanted)
    {
        let path = ctx.project_root.join(&file.path);
        return ctx.parts.should_index(&path).then_some(path);
    }
    let candidate = ctx.project_root.join(&wanted);
    if candidate.is_file()
        && candidate.extension().and_then(|e| e.to_str()) == Some(jet_driver::Syntax::FILE_EXT)
        && project_source_roots(&ctx)
            .iter()
            .any(|root| canonical_path(&candidate).starts_with(canonical_path(root)))
        && ctx.parts.should_index(&candidate)
    {
        return Some(candidate);
    }
    None
}

fn resolve_entry_source_path(entry: &Path, source_id: Option<&str>) -> Result<PathBuf, String> {
    let Some(id) = source_id else {
        return Ok(entry.to_path_buf());
    };
    project_path_for_source_id(entry, id).ok_or_else(|| {
        query_error(
            "not_found",
            "Canvas source_id must name a projected Jet source file",
        )
    })
}

fn project_source_roots(ctx: &ProjectContext) -> Vec<PathBuf> {
    if let Some(workspace_root) = &ctx.workspace_root {
        if let Some(Ok(snapshot)) = jet_env_model::WorkspaceFile::load_checked(workspace_root) {
            return snapshot
                .plan
                .members
                .into_iter()
                .map(|member| workspace_root.join(member.path))
                .collect();
        }
    }
    if let Some(root) = &ctx.manifest_root {
        return vec![root.clone()];
    }
    if let Some(root) = &ctx.ecosystem_root {
        return vec![root.clone()];
    }
    vec![ctx
        .entry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()]
}

fn project_capabilities_json(
    packages: &str,
    targets: &str,
    environment: &EnvProjectJson,
) -> String {
    let target_text = format!("{packages}{targets}").to_ascii_lowercase();
    let service = !environment.services.is_empty()
        || target_text.contains("\"kind\":\"service\"")
        || target_text.contains("\"target\":\"service\"");
    let has_checked_output = [
        "\"kind\":\"library\"",
        "\"kind\":\"executable\"",
        "\"kind\":\"service\"",
        "\"kind\":\"check\"",
        "\"kind\":\"environment\"",
        "\"kind\":\"image\"",
        "\"kind\":\"bundle\"",
        "\"kind\":\"system\"",
        "\"kind\":\"fleet\"",
    ]
    .iter()
    .any(|kind| target_text.contains(kind));
    let runtime_output = !has_checked_output
        || target_text.contains("\"kind\":\"executable\"")
        || target_text.contains("\"kind\":\"service\"")
        || service;
    let preview = target_text.contains("\"target\":\"web\"")
        || target_text.contains("\"kind\":\"ui\"")
        || target_text.contains("\"kind\":\"game\"")
        || target_text.contains("\"target\":\"ui\"")
        || target_text.contains("\"target\":\"game\"");
    let designer = target_text.contains("\"kind\":\"ui\"")
        || target_text.contains("\"kind\":\"game\"")
        || target_text.contains("\"target\":\"ui\"")
        || target_text.contains("\"target\":\"game\"");
    let mut capabilities = vec!["\"graph\":true", "\"code\":true", "\"diagnostics\":true"];
    if runtime_output {
        capabilities.push("\"runtime_output\":true");
    }
    if runtime_output && !preview {
        capabilities.push("\"terminal\":true");
    }
    if service {
        capabilities.push("\"service\":true");
    }
    if preview {
        capabilities.push("\"preview\":true");
    }
    if designer {
        capabilities.push("\"designer\":true");
    }
    format!("{{{}}}", capabilities.join(","))
}

/// Project package/workspace source truth into the public Canvas project schema.
pub fn project_json_for_entry(path: &Path) -> String {
    // Parent-walk discovery can parse `.jet` files; use the compiler stack +
    // TIR bridge rather than the thin test/UI thread (default ~2MiB).
    let payload = jet_driver::run_compiler_work(|| project_json_for_entry_inner(path));
    canvas_machine_success("canvas.project", &payload)
}

fn project_json_for_entry_inner(path: &Path) -> String {
    let ctx = project_context_for_entry(path);
    let entry_rel = rel_path(&ctx.project_root, path);
    let workspace_json = workspace_project_json(&ctx.project_root, ctx.workspace_root.as_deref());
    let packages_json = packages_project_json(
        &ctx.project_root,
        &ctx.entry_path,
        ctx.manifest_root.as_deref(),
        ctx.ecosystem_root.as_deref(),
        ctx.workspace_root.as_deref(),
    );
    let targets_json = targets_project_json(
        &ctx.project_root,
        &ctx.entry_path,
        ctx.manifest_root.as_deref(),
        ctx.ecosystem_root.as_deref(),
        ctx.workspace_root.as_deref(),
    );
    let files_json = ctx
        .files
        .iter()
        .map(|f| {
            format!(
                "{{\"path\":{},\"revision\":{},\"kind\":{}}}",
                json_str(&f.path),
                json_str(&f.revision),
                json_str(&f.kind)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let parts_json = ctx
        .parts
        .parts
        .iter()
        .map(|part| {
            format!(
                "{{\"name\":{},\"path\":{},\"state\":{}}}",
                json_str(&part.canonical_name()),
                json_str(&rel_path(&ctx.project_root, &part.path)),
                json_str(part.state.name())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let part_conflicts_json = ctx
        .parts
        .conflicts
        .iter()
        .map(|conflict| {
            let paths = conflict
                .paths
                .iter()
                .map(|path| json_str(&rel_path(&ctx.project_root, path)))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"name\":{},\"paths\":[{}]}}",
                json_str(&conflict.canonical_name()),
                paths
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let locks_json = lock_project_json(&ctx.project_root);
    let env_projection = env_project_json(&ctx.project_root);
    let capabilities_json =
        project_capabilities_json(&packages_json, &targets_json, &env_projection);
    let diagnostics = [
        ctx.authority_diagnostic
            .as_ref()
            .map(diagnostic_json)
            .unwrap_or_default(),
        env_projection.diagnostics.clone(),
    ]
    .into_iter()
    .filter(|diagnostic| !diagnostic.is_empty())
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"protocol\":\"jet.canvas.project\",\"schema_version\":{},\"project_root\":{},\"project_revision\":{},\"entry\":{},\"mode\":{},\"capabilities\":{},\"workspace\":{},\"packages\":[{}],\"targets\":[{}],\"outputs\":[{}],\"envs\":[{}],\"services\":[{}],\"files\":[{}],\"parts\":[{}],\"part_conflicts\":[{}],\"locks\":[{}],\"diagnostics\":[{}],\"source_control\":{{\"truth\":\"git-text\"}},\"state_policy\":{{\"semantic\":\"source\",\"local\":[\"tabs\",\"viewport\",\"selection\",\"breakpoints\",\"watches\",\"comment_boxes\",\"staged_nodes\"],\"shared_visual\":\"source-anchored-comments\"}}}}",
        PROJECT_SCHEMA_VERSION,
        json_str(&ctx.project_root.display().to_string()),
        json_str(&ctx.project_revision),
        json_str(&entry_rel),
        json_str(if ctx.workspace_root.is_some() {
            "workspace"
        } else if ctx.manifest_root.is_some() || ctx.ecosystem_root.is_some() {
            "package"
        } else {
            "single_file"
        }),
        capabilities_json,
        workspace_json,
        packages_json,
        targets_json,
        targets_json.clone(),
        env_projection.envs,
        env_projection.services,
        files_json,
        parts_json,
        part_conflicts_json,
        locks_json,
        diagnostics
    )
}

/// Apply one versioned package/workspace transaction and write source-truth files.
pub fn apply_project_transaction_json(path: &Path, request: &str) -> Result<String, String> {
    jet_driver::run_compiler_work(|| apply_project_transaction_json_inner(path, request))
        .map(|payload| canvas_machine_success("canvas.project.edit", &payload))
        .map_err(|error| canvas_machine_error("canvas.project.edit", &error))
}

fn apply_project_transaction_json_inner(path: &Path, request: &str) -> Result<String, String> {
    let ctx = project_context_for_entry(path);
    if let Some(diagnostic) = ctx.authority_diagnostic.as_ref() {
        return Err(project_edit_error("stale", &diagnostic.what));
    }
    let schema = json_usize_field(request, "schema_version").unwrap_or(0);
    if schema != PROJECT_SCHEMA_VERSION as usize {
        return Err(project_edit_error(
            "schema",
            "Canvas project transaction schema_version must be 1",
        ));
    }
    let project_revision = required_project_string(request, "project_revision")?;
    if project_revision != ctx.project_revision {
        return Err(project_edit_conflict(
            "project changed since this Canvas transaction was drawn",
            &ctx.project_revision,
        ));
    }
    let op = required_project_string(request, "op")?;
    let touched = required_project_touched_files(request)?;
    validate_touched_project_files(&ctx, &touched)?;
    match op.as_str() {
        "add_dependency" => apply_project_add_dependency(&ctx, request, &touched),
        "remove_dependency" => apply_project_remove_dependency(&ctx, request, &touched),
        "edit_pkg_field" => apply_project_edit_pkg_field(&ctx, request, &touched),
        "add_target" => apply_project_add_target(&ctx, request, &touched),
        "create_package" => apply_project_create_package(&ctx, request, &touched),
        "add_workspace_member" => apply_project_add_workspace_member(&ctx, request, &touched),
        "add_env_service" => apply_project_add_env_service(&ctx, request, &touched),
        "rename_binding" | "rename_function" => apply_project_rename(&ctx, request, &touched),
        _ => Err(project_edit_error(
            "unsupported",
            "unknown Canvas project transaction operation",
        )),
    }
}

/// Query Canvas graph/source facts through the same semindex data LSP consumes.
pub fn query_json_for_file(path: &Path, request: &str) -> Result<String, String> {
    jet_driver::run_compiler_work(|| query_json_for_file_inner(path, request))
        .map(|payload| canvas_machine_success("canvas.query", &payload))
        .map_err(|error| canvas_machine_error("canvas.query", &error))
}

fn query_json_for_file_inner(path: &Path, request: &str) -> Result<String, String> {
    let src = fs::read_to_string(path).map_err(|e| query_error("io", &e.to_string()))?;
    let revision = required_query_string(request, "revision")?;
    if revision != source_revision(&src) {
        return Err(query_error(
            "conflict",
            "source changed since this Canvas graph was drawn",
        ));
    }
    let schema = json_usize_field(request, "schema_version").unwrap_or(0);
    if schema != QUERY_SCHEMA_VERSION as usize {
        return Err(query_error(
            "schema",
            "Canvas query schema_version must be 1",
        ));
    }
    let op = required_query_string(request, "op")?;
    match op.as_str() {
        "find" | "project_search" => {
            let query = required_query_string(request, "query")?;
            canvas_find(path, &src, &query)
        }
        "references" => {
            let symbol = required_query_string(request, "symbol")?;
            canvas_references(path, &src, &symbol)
        }
        "source_to_graph" => {
            let start = json_usize_field(request, "start").unwrap_or(0);
            let end = json_usize_field(request, "end").unwrap_or(start);
            canvas_source_to_graph(path, &src, SourceSpan { start, end })
        }
        "preview_rename" => {
            let symbol = required_query_string(request, "symbol")?;
            let to = required_query_string(request, "to")?;
            validate_query_ident(&to)?;
            canvas_preview_rename(path, &src, &symbol, &to)
        }
        "actions" | "palette_entries" => canvas_actions(path, &src),
        "core_catalog" | "corelib_catalog" => canvas_core_catalog_query(path, &src, request),
        _ => Err(query_error("unsupported", "unknown Canvas query operation")),
    }
}

/// Query graph/source facts for a selected file inside the entry project.
pub fn query_json_for_entry(entry: &Path, request: &str) -> Result<String, String> {
    jet_driver::run_compiler_work(|| query_json_for_entry_inner(entry, request))
        .map(|payload| canvas_machine_success("canvas.query", &payload))
        .map_err(|error| canvas_machine_error("canvas.query", &error))
}

fn query_json_for_entry_inner(entry: &Path, request: &str) -> Result<String, String> {
    let source_id = json_string_field(request, "source_id");
    let path = resolve_entry_source_path(entry, source_id.as_deref())?;
    let schema = json_usize_field(request, "schema_version").unwrap_or(0);
    if schema != QUERY_SCHEMA_VERSION as usize {
        return Err(query_error(
            "schema",
            "Canvas query schema_version must be 1",
        ));
    }
    let op = json_string_field(request, "op");
    let expected_project_revision = json_string_field(request, "project_revision");
    if op.as_deref() == Some("project_search") {
        let src = fs::read_to_string(&path).map_err(|e| query_error("io", &e.to_string()))?;
        let revision = required_query_string(request, "revision")?;
        if revision != source_revision(&src) {
            return Err(query_error(
                "conflict",
                "source changed since this Canvas graph was drawn",
            ));
        }
        return canvas_project_find(
            entry,
            &path,
            &src,
            &required_query_string(request, "query")?,
            expected_project_revision.as_deref(),
        );
    }
    if op.as_deref() == Some("references") {
        let src = fs::read_to_string(&path).map_err(|e| query_error("io", &e.to_string()))?;
        let revision = required_query_string(request, "revision")?;
        if revision != source_revision(&src) {
            return Err(query_error(
                "conflict",
                "source changed since this Canvas graph was drawn",
            ));
        }
        return canvas_project_references(
            entry,
            &path,
            &src,
            &required_query_string(request, "symbol")?,
            expected_project_revision.as_deref(),
        );
    }
    if op.as_deref() == Some("preview_rename") && expected_project_revision.is_some() {
        let src = fs::read_to_string(&path).map_err(|e| query_error("io", &e.to_string()))?;
        let revision = required_query_string(request, "revision")?;
        if revision != source_revision(&src) {
            return Err(query_error(
                "conflict",
                "source changed since this Canvas graph was drawn",
            ));
        }
        let symbol = required_query_string(request, "symbol")?;
        let to = required_query_string(request, "to")?;
        validate_query_ident(&to)?;
        return canvas_project_preview_rename(
            entry,
            &path,
            &src,
            &symbol,
            &to,
            expected_project_revision.as_deref(),
        );
    }
    query_json_for_file_inner(&path, request)
}

/// Expose the canonical Core library catalog to Canvas without granting
/// execution authority.
pub fn core_catalog_json_for_entry(entry: &Path, query: &str) -> Result<String, String> {
    let result = (|| {
        let path = resolve_entry_source_path(entry, None)?;
        let src = fs::read_to_string(&path).map_err(|e| query_error("io", &e.to_string()))?;
        canvas_core_catalog(&path, &src, query)
    })();
    result
        .map(|payload| canvas_machine_success("canvas.core_catalog", &payload))
        .map_err(|error| canvas_machine_error("canvas.core_catalog", &error))
}

/// Report Git text truth for Canvas source-control UI.
pub fn source_control_json_for_file(path: &Path) -> String {
    let payload = source_control_json_for_file_inner(path);
    canvas_machine_success("canvas.source_control", &payload)
}

fn source_control_json_for_file_inner(path: &Path) -> String {
    let src = fs::read_to_string(path).unwrap_or_default();
    let semantic_ops = semantic_ops_json(path, &src);
    let Some(root) = git_root(path) else {
        return format!(
            "{{\"protocol\":\"jet.canvas.source_control\",\"schema_version\":1,\"ok\":true,\"revision\":{},\"available\":false,\"dirty\":false,\"status\":{},\"diff\":{},\"history\":[],\"semantic_ops\":[{}]}}",
            json_str(&source_revision(&src)),
            json_str("not a Git worktree"),
            json_str(""),
            semantic_ops
        );
    };
    let rel = git_relative_path(&root, path);
    let status = git_output(&root, &["status", "--porcelain", "--", &rel]).unwrap_or_default();
    let tracked = git_output(&root, &["ls-files", "--error-unmatch", "--", &rel]).is_some();
    let diff = if tracked {
        git_output(&root, &["diff", "--", &rel]).unwrap_or_default()
    } else if path.exists() {
        untracked_diff(&rel, &src)
    } else {
        String::new()
    };
    let log = git_output(&root, &["log", "--oneline", "-5", "--", &rel]).unwrap_or_default();
    let history = log
        .lines()
        .map(|line| json_str(line))
        .collect::<Vec<_>>()
        .join(",");
    let dirty = !status.trim().is_empty() || !diff.trim().is_empty();
    format!(
        "{{\"protocol\":\"jet.canvas.source_control\",\"schema_version\":1,\"ok\":true,\"revision\":{},\"available\":true,\"dirty\":{},\"status\":{},\"diff\":{},\"history\":[{}],\"semantic_ops\":[{}]}}",
        json_str(&source_revision(&src)),
        if dirty { "true" } else { "false" },
        json_str(status.trim()),
        json_str(&diff),
        history,
        semantic_ops
    )
}

fn semantic_ops_json(path: &Path, source: &str) -> String {
    let source_hash = SHA256::sha256_hex(source.as_bytes());
    semantic_ops_for_file(path, &source_hash)
        .iter()
        .map(semantic_op_json)
        .collect::<Vec<_>>()
        .join(",")
}

fn semantic_op_json(op: &SemanticOp) -> String {
    let optional = |value: &Option<String>| {
        value
            .as_deref()
            .map(json_str)
            .unwrap_or_else(|| "null".to_string())
    };
    let targets = op
        .targets
        .iter()
        .map(|target| {
            format!(
                "{{\"stable_id\":{},\"before\":{},\"after\":{},\"kind\":{},\"module_path\":{}}}",
                json_str(&target.stable_id),
                json_str(&target.before),
                json_str(&target.after),
                json_str(&target.kind),
                json_str(&target.module_path),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":{},\"rule_id\":{},\"from\":{},\"to\":{},\"node\":{},\"match\":{},\"replace\":{},\"targets\":[{}]}}",
        json_str(&op.kind),
        optional(&op.rule_id),
        optional(&op.from),
        optional(&op.to),
        optional(&op.node),
        optional(&op.match_template),
        optional(&op.replace_template),
        targets,
    )
}

/// Report Git text truth for the whole projected package/workspace.
pub fn source_control_json_for_entry(path: &Path) -> String {
    let payload = jet_driver::run_compiler_work(|| source_control_json_for_entry_inner(path));
    canvas_machine_success("canvas.source_control", &payload)
}

fn source_control_json_for_entry_inner(path: &Path) -> String {
    let ctx = project_context_for_entry(path);
    let git_root = git_root(path);
    let mut dirty_files = 0;
    let mut statuses = Vec::new();
    let files = ctx
        .files
        .iter()
        .map(|file| {
            let abs = ctx.project_root.join(&file.path);
            let src = fs::read_to_string(&abs).unwrap_or_default();
            let mut status = String::new();
            let mut diff = String::new();
            let mut dirty = false;
            if let Some(root) = git_root.as_deref() {
                let git_rel = git_relative_path(root, &abs);
                status = git_output(root, &["status", "--porcelain", "--", &git_rel])
                    .unwrap_or_default();
                let tracked = git_output(root, &["ls-files", "--error-unmatch", "--", &git_rel])
                    .is_some();
                diff = if tracked {
                    git_output(root, &["diff", "--", &git_rel]).unwrap_or_default()
                } else if abs.exists() {
                    untracked_diff(&git_rel, &src)
                } else {
                    String::new()
                };
                dirty = !status.trim().is_empty() || !diff.trim().is_empty();
                if !status.trim().is_empty() {
                    statuses.push(status.trim().to_string());
                }
            }
            if dirty {
                dirty_files += 1;
            }
            let semantic_ops = semantic_ops_json(&abs, &src);
            format!(
                "{{\"path\":{},\"revision\":{},\"kind\":{},\"available\":{},\"dirty\":{},\"status\":{},\"diff\":{},\"semantic_ops\":[{}]}}",
                json_str(&file.path),
                json_str(&file.revision),
                json_str(&file.kind),
                if git_root.is_some() { "true" } else { "false" },
                if dirty { "true" } else { "false" },
                json_str(status.trim()),
                json_str(&diff),
                semantic_ops
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let entry_src = fs::read_to_string(path).unwrap_or_default();
    let history = git_root
        .as_deref()
        .and_then(|root| {
            let rel = git_relative_path(root, path);
            git_output(root, &["log", "--oneline", "-5", "--", &rel])
        })
        .unwrap_or_default()
        .lines()
        .map(json_str)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"protocol\":\"jet.canvas.source_control\",\"schema_version\":1,\"ok\":true,\"revision\":{},\"project_revision\":{},\"project_root\":{},\"available\":{},\"dirty\":{},\"dirty_files\":{},\"status\":{},\"diff\":{},\"history\":[{}],\"files\":[{}]}}",
        json_str(&source_revision(&entry_src)),
        json_str(&ctx.project_revision),
        json_str(&ctx.project_root.display().to_string()),
        if git_root.is_some() { "true" } else { "false" },
        if dirty_files > 0 { "true" } else { "false" },
        dirty_files,
        json_str(&statuses.join("\n")),
        json_str(""),
        history,
        files
    )
}

/// Report local proof truth for the selected source without pretending a run/build
/// receipt exists.
pub fn proof_json_for_entry(entry: &Path, source_id: Option<&str>) -> Result<String, String> {
    proof_json_for_entry_with_receipt(entry, source_id, None)
}

pub fn proof_json_for_entry_with_receipt(
    entry: &Path,
    source_id: Option<&str>,
    command_receipt: Option<&str>,
) -> Result<String, String> {
    proof_json_for_entry_with_receipt_inner(entry, source_id, command_receipt)
        .map(|payload| canvas_machine_success("canvas.proof", &payload))
        .map_err(|error| canvas_machine_error("canvas.proof", &error))
}

fn proof_json_for_entry_with_receipt_inner(
    entry: &Path,
    source_id: Option<&str>,
    command_receipt: Option<&str>,
) -> Result<String, String> {
    let path = resolve_entry_source_path(entry, source_id)?;
    let src = fs::read_to_string(&path).map_err(|e| query_error("io", &e.to_string()))?;
    let revision = source_revision(&src);
    let check = match project_file(&path) {
        Ok(_) => {
            "{\"state\":\"ok\",\"diagnostics_count\":0,\"message\":\"front end check passed\"}"
                .to_string()
        }
        Err(diags) => {
            let message =
                jet_driver::Diagnostics::render_all(&path.display().to_string(), &src, &diags);
            format!(
                "{{\"state\":\"diagnostic\",\"diagnostics_count\":{},\"message\":{}}}",
                diags.len(),
                json_str(&message)
            )
        }
    };
    let (git_available, git_dirty, git_status) = if let Some(root) = git_root(&path) {
        let rel = git_relative_path(&root, &path);
        let status = git_output(&root, &["status", "--porcelain", "--", &rel]).unwrap_or_default();
        let tracked = git_output(&root, &["ls-files", "--error-unmatch", "--", &rel]).is_some();
        let diff = if tracked {
            git_output(&root, &["diff", "--", &rel]).unwrap_or_default()
        } else if path.exists() {
            untracked_diff(&rel, &src)
        } else {
            String::new()
        };
        (
            "true",
            if !status.trim().is_empty() || !diff.trim().is_empty() {
                "true"
            } else {
                "false"
            },
            status.trim().to_string(),
        )
    } else {
        ("false", "false", "not a Git worktree".to_string())
    };
    let source_label = source_id
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string());
    let receipt_current = command_receipt
        .map(|receipt| receipt.contains(&format!("\"revision\":\"{revision}\"")))
        .unwrap_or(false);
    let command_receipts = if receipt_current {
        format!(
            "{{\"state\":\"current\",\"receipt\":{}}}",
            command_receipt.unwrap_or("{}")
        )
    } else {
        "{\"state\":\"missing\",\"reason\":\"no Canvas command authority receipt has run for this source revision\"}".to_string()
    };
    let proof = if receipt_current {
        "{\"state\":\"current\",\"stale\":false,\"reasons\":[]}".to_string()
    } else {
        "{\"state\":\"missing\",\"stale\":true,\"reasons\":[\"no check/build/run receipt for this source revision\"]}".to_string()
    };
    let budget_root =
        jet_driver::Loader::find_manifest_root(path.parent().unwrap_or(Path::new(".")))
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
    let budget_path = path
        .strip_prefix(&budget_root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/");
    let budget_digest = jet_driver::SHA256::sha256_hex(src.as_bytes());
    let budgets =
        jet_driver::BudgetView::read_compatible(&budget_root, &[(budget_path, budget_digest)]);
    let budget_reports = budgets.facts.iter().map(|fact| format!(
        "{{\"budget_id\":{},\"enforcement\":{},\"evidence\":{},\"evidence_id\":{},\"outcome\":{},\"report_id\":{},\"statistical\":{}}}",
        json_str(&fact.budget_id), json_str(&fact.enforcement), json_str(&fact.evidence), json_str(&fact.evidence_id),
        json_str(&fact.outcome), json_str(&fact.report_id), fact.statistical
    )).collect::<Vec<_>>().join(",");
    let budget_rejected = budgets
        .rejected
        .iter()
        .map(|reason| json_str(reason))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"protocol\":\"jet.canvas.proof\",\"schema_version\":{},\"ok\":true,\"source_id\":{},\"source_path\":{},\"revision\":{},\"check\":{},\"source_control\":{{\"truth\":\"git-text\",\"available\":{},\"dirty\":{},\"status\":{}}},\"debug\":{{\"state\":\"local-only\",\"persistence\":\"local-source-span\"}},\"command_receipts\":{},\"budget_reports\":{{\"mode\":\"read_only\",\"reports\":[{}],\"rejected\":[{}]}},\"proof\":{}}}",
        PROOF_SCHEMA_VERSION,
        json_str(&source_label),
        json_str(&path.display().to_string()),
        json_str(&revision),
        check,
        git_available,
        git_dirty,
        json_str(&git_status),
        command_receipts,
        budget_reports,
        budget_rejected,
        proof
    ))
}

pub fn command_receipt_json_for_entry(entry: &Path, request: &str) -> Result<String, String> {
    command_receipt_json_for_entry_inner(entry, request)
        .map(|payload| canvas_machine_success("canvas.command_receipt", &payload))
        .map_err(|error| canvas_machine_error("canvas.command_receipt", &error))
}

fn command_receipt_json_for_entry_inner(entry: &Path, request: &str) -> Result<String, String> {
    let source_id = json_string_field(request, "source_id");
    let source_path = resolve_entry_source_path(entry, source_id.as_deref())?;
    let src = fs::read_to_string(&source_path).map_err(|e| query_error("io", &e.to_string()))?;
    let revision = required_string(request, "revision")?;
    if revision != source_revision(&src) {
        return Err(query_error(
            "conflict",
            "source changed since this Canvas command was approved",
        ));
    }
    let action_id = required_string(request, "action_id")?;
    let source = source_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(jet_driver::Syntax::DEFAULT_ENTRY_FILE);
    let source_label = source_id.clone().unwrap_or_else(|| {
        let ctx = project_context_for_entry(entry);
        rel_path(&ctx.project_root, &source_path)
    });
    let (label, args, writes, requires_confirmation) = match action_id.as_str() {
        "canvas.command:run" => ("Run program", vec!["run", source], "none", false),
        "canvas.command:check" => ("Check project", vec!["check", source], "none", false),
        "canvas.command:build" => (
            "Build project",
            vec!["build", source],
            "build_outputs",
            true,
        ),
        _ => {
            return Err(query_error(
                "unsupported",
                "Canvas command execution only supports run, check, and build receipts",
            ))
        }
    };
    if requires_confirmation && !json_bool_field(request, "confirmed").unwrap_or(false) {
        return Err(query_error(
            "confirmation_required",
            "this Canvas command writes build outputs and needs confirmed:true",
        ));
    }
    let started = std::time::Instant::now();
    let source_override = json_string_field(request, "source_text");
    let check_src = source_override.as_deref().unwrap_or(&src);
    let check_revision = source_revision(check_src);
    let (success, exit_code, stdout, stderr, diagnostics) = if action_id == "canvas.command:check" {
        let abs = canonical_path(&source_path);
        let overlay = source_override.as_deref().map(|text| (abs.as_path(), text));
        let (diags, _bundle, _facts) = jet_driver::Driver::check_file_with_effect_facts(
            &source_path.display().to_string(),
            overlay,
            true,
        );
        let errors: Vec<Diagnostic> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .cloned()
            .collect();
        if errors.is_empty() {
            (true, Some(0), String::new(), String::new(), String::new())
        } else {
            (
                false,
                Some(1),
                String::new(),
                jet_driver::Diagnostics::render_all(
                    &source_path.display().to_string(),
                    check_src,
                    &errors,
                ),
                diagnostics_json(&source_path, check_src, &errors),
            )
        }
    } else {
        let output = run_jet_command(&source_path, &args)?;
        (
            output.status.success(),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
            String::new(),
        )
    };
    let elapsed_ms = started.elapsed().as_millis();
    let mut display = vec!["jet".to_string()];
    display.extend(args.iter().map(|arg| arg.to_string()));
    let command = display
        .iter()
        .map(|arg| json_str(arg))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"protocol\":\"jet.canvas.command_receipt\",\"schema_version\":1,\"ok\":true,\"action_id\":{},\"title\":{},\"source_id\":{},\"revision\":{},\"checked_revision\":{},\"command\":[{}],\"writes\":{},\"success\":{},\"exit_code\":{},\"elapsed_ms\":{},\"stdout\":{},\"stderr\":{},\"diagnostics\":[{}]}}",
        json_str(&action_id),
        json_str(label),
        json_str(&source_label),
        json_str(&revision),
        json_str(&check_revision),
        command,
        json_str(writes),
        if success { "true" } else { "false" },
        exit_code.map(|c| c.to_string()).unwrap_or_else(|| "null".to_string()),
        elapsed_ms,
        json_str(&stdout),
        json_str(&stderr),
        diagnostics
    ))
}

fn run_jet_command(entry: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let cwd = entry.parent().unwrap_or_else(|| Path::new("."));
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(output) = Command::new(&exe).args(args).current_dir(cwd).output() {
            return Ok(output);
        }
    }
    Command::new("jet")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| query_error("io", &e.to_string()))
}

/// Apply one versioned Canvas edit transaction and write ordinary Jet source.
pub fn apply_transaction_json(path: &Path, request: &str) -> Result<String, String> {
    apply_transaction_json_inner(path, request)
        .map(|payload| canvas_machine_success("canvas.edit", &payload))
        .map_err(|error| canvas_machine_error("canvas.edit", &error))
}

fn apply_transaction_json_inner(path: &Path, request: &str) -> Result<String, String> {
    let path = match json_string_field(request, "source_id") {
        Some(source_id) => project_path_for_source_id(path, &source_id).ok_or_else(|| {
            edit_error(
                "not_found",
                "Canvas source_id must name a projected Jet source file",
            )
        })?,
        None => path.to_path_buf(),
    };
    edit_transaction::apply_transaction_json(&path, request)
}

/// Run the debugger against the source selected by a project-relative id.
pub fn debug_session_json_for_entry(entry: &Path, request: &str) -> Result<String, String> {
    let sessions = DebugSessions::one_shot();
    debug_session_json_for_entry_with_sessions(entry, request, &sessions)
}

/// Run a debugger command against the shared live-session store owned by the
/// dev server. The public one-shot wrapper above remains useful to embedders
/// that do not own a server lifetime.
pub fn debug_session_json_for_entry_with_sessions(
    entry: &Path,
    request: &str,
    sessions: &DebugSessions,
) -> Result<String, String> {
    let source_id = json_string_field(request, "source_id");
    let path = match resolve_entry_source_path(entry, source_id.as_deref()) {
        Ok(path) => path,
        Err(error) => return Err(canvas_machine_error("canvas.debug", &error)),
    };
    debug_session_json_for_file_with_sessions(&path, request, sessions)
}

/// Run one source-level debugger slice and project it onto Canvas graph spans.
pub fn debug_session_json_for_file(path: &Path, request: &str) -> Result<String, String> {
    let sessions = DebugSessions::one_shot();
    debug_session_json_for_file_with_sessions(path, request, &sessions)
}

pub fn debug_session_json_for_file_with_sessions(
    path: &Path,
    request: &str,
    sessions: &DebugSessions,
) -> Result<String, String> {
    debug_session_json_for_file_with_sessions_inner(path, request, sessions)
        .map(|payload| canvas_machine_success("canvas.debug", &payload))
        .map_err(|error| canvas_machine_error("canvas.debug", &error))
}

fn debug_session_json_for_file_with_sessions_inner(
    path: &Path,
    request: &str,
    sessions: &DebugSessions,
) -> Result<String, String> {
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(error) => {
            if let Some(id) = json_string_field(request, "session_id") {
                sessions.discard(&id);
            }
            return Err(debug_error("io", &error.to_string()));
        }
    };
    let revision = required_debug_string(request, "revision")?;
    let schema = json_usize_field(request, "schema_version").unwrap_or(0);
    if schema != DEBUG_SCHEMA_VERSION as usize {
        return Err(debug_error(
            "schema",
            "Canvas debug schema_version must be 1",
        ));
    }
    let session_id = json_string_field(request, "session_id");
    let requested_tier = DebugTier::parse(json_string_field(request, "tier").as_deref())?;
    if json_bool_field(request, "stop").unwrap_or(false) {
        let id = required_debug_string(request, "session_id")?;
        let current_revision = source_revision(&src);
        let tier = sessions.stop(path, &revision, &current_revision, &id, requested_tier)?;
        let source_id = json_string_field(request, "source_id").unwrap_or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "source".to_string())
        });
        return Ok(debug_stop_ok(&src, &id, tier, &source_id));
    }
    if revision != source_revision(&src) {
        if let Some(id) = session_id.as_deref() {
            sessions.discard(id);
        }
        return Err(debug_error(
            "conflict",
            "source changed since this Canvas debug state was drawn",
        ));
    }

    let mut breakpoint_lines = json_usize_array(request, "breakpoints");
    let mut stale_breakpoints = Vec::new();
    let breakpoint_anchors = json_string_array(request, "breakpoint_spans");
    validate_debug_breakpoint_anchors(&breakpoint_anchors)?;
    for anchor in breakpoint_anchors {
        if let Some(line) = line_from_anchor(&src, &anchor) {
            breakpoint_lines.push(line);
        } else {
            stale_breakpoints.push(anchor);
        }
    }
    breakpoint_lines.retain(|line| {
        if *line > 0 && *line <= src.lines().count() {
            true
        } else {
            stale_breakpoints.push(format!("line:{line}"));
            false
        }
    });
    breakpoint_lines.sort_unstable();
    breakpoint_lines.dedup();
    let watches = json_string_array(request, "watches");
    let commands = json_string_array(request, "commands");
    let execution = sessions.execute(
        path,
        &revision,
        session_id.as_deref(),
        &commands,
        &breakpoint_lines,
        &watches,
        requested_tier,
    )?;
    if execution.status == jet_debug::SessionStatus::Failed {
        return Err(debug_error("diagnostic", &execution.transcript));
    }
    let current_src = fs::read_to_string(path)
        .map_err(|error| debug_error("io", &format!("couldn't re-read debug source: {error}")))?;
    if current_src != src {
        if execution.status == jet_debug::SessionStatus::Running {
            sessions.discard(&execution.id);
        }
        return Err(debug_error(
            "conflict",
            "source changed while this Canvas debug command was running; the source was kept",
        ));
    }

    let projection = match project_file(path) {
        Ok(projection) => projection,
        Err(diags) => {
            sessions.discard(&execution.id);
            return Err(debug_diagnostics_error(path, &src, &diags));
        }
    };
    let source_id = json_string_field(&projection.json, "source_id").unwrap_or_else(|| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "source".to_string())
    });
    Ok(debug_ok(
        &src,
        &projection.json,
        &execution.transcript,
        execution.snapshot.as_ref(),
        execution.status,
        &execution.id,
        execution.tier,
        &source_id,
        &breakpoint_lines,
        &stale_breakpoints,
        &watches,
    ))
}
