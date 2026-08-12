use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jet_driver::Diagnostics::{Diagnostic, Severity};
use jet_driver::SHA256;
use jet_semindex::SourceSpan;

use super::debug_source_git::{
    canonical_path, debug_diagnostics_error, debug_error, debug_ok, git_output,
    git_relative_path, git_root, line_from_anchor, required_debug_string, untracked_diff,
};
use super::edit_actions::{
    apply_add_pattern_arm, apply_append_multi_input, apply_break_link,
    apply_create_collapse_region, apply_create_comment_region, apply_create_function,
    apply_create_trait_impl, apply_delete_comment_region, apply_delete_hint_region,
    apply_edit_function_signature, apply_edit_pattern_arm, apply_inline_edit, apply_insert_call,
    apply_insert_structural, apply_move_link, apply_noop, apply_promote_inline,
    apply_remove_multi_input_element, apply_remove_pattern_arm, apply_rename,
    apply_reorder_statements, apply_update_comment_region, apply_visible_conversion,
    canvas_action_candidate, extract_inline_candidate, inline_helper_candidate,
    write_checked_formatted, write_checked_source,
};
use super::graph_helpers::{
    canvas_action_preview_ok, diagnostics_json, edit_error, preview_ok, project_edit_error,
    query_error,
};
use super::project_scan::{
    ProjectContext, env_project_json, lock_project_json, packages_project_json,
    project_context_for_entry, project_file, project_file_with_runtime, targets_project_json,
    workspace_project_json,
};
use super::project_transactions::{
    apply_project_add_dependency, apply_project_add_env_service, apply_project_add_target,
    apply_project_add_workspace_member, apply_project_create_package, apply_project_edit_pkg_field,
    apply_project_remove_dependency, clean_project_rel_path, rel_path,
    diagnostic_json, required_project_touched_files, validate_touched_project_files,
};
use super::query_actions::{
    canvas_actions, canvas_core_catalog, canvas_core_catalog_query, canvas_find,
    canvas_preview_rename, canvas_references, canvas_source_to_graph,
};
use super::validation_json::{
    json_bool_field, json_str, json_string_array, json_string_field, json_usize_array,
    json_usize_field, required_project_string, required_query_string, required_string,
    validate_function_signature, validate_ident, validate_qualified_name, validate_query_ident,
    validate_signature_fragment, validate_single_line_fragment, validate_type_fragment,
};

pub const GRAPH_SCHEMA_VERSION: u32 = 1;
pub const EDIT_SCHEMA_VERSION: u32 = 1;
pub const DEBUG_SCHEMA_VERSION: u32 = 1;
pub const QUERY_SCHEMA_VERSION: u32 = 1;
pub const ACTION_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const CORE_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const PROOF_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(super) struct InlineExpr {
    pub(super) id: String,
    pub(super) span: SourceSpan,
}

#[derive(Debug, Clone)]
pub(super) struct GraphEditAnchor {
    pub(super) graph_id: String,
    pub(super) insert_offset: usize,
    pub(super) fallible: bool,
}

#[derive(Debug, Clone)]
pub(super) struct NodeQueryRef {
    pub(super) graph_id: String,
    pub(super) node_id: String,
    pub(super) kind: String,
    pub(super) title: String,
    pub(super) span: SourceSpan,
}

#[derive(Debug, Clone)]
pub(super) struct Projection {
    pub(super) json: String,
    pub(super) inline_exprs: Vec<InlineExpr>,
    pub(super) graph_anchors: Vec<GraphEditAnchor>,
    pub(super) node_refs: Vec<NodeQueryRef>,
}

#[derive(Default)]
pub(super) struct GraphBuilder {
    pub(super) graph_id: String,
    pub(super) nodes: Vec<NodeRec>,
    pub(super) pins: Vec<PinRec>,
    pub(super) wires: Vec<WireRec>,
    pub(super) regions: Vec<String>,
    pub(super) inline_exprs: Vec<InlineRec>,
    pub(super) local_pins: HashMap<String, String>,
    pub(super) local_types: HashMap<String, String>,
    pub(super) getter_pins: HashMap<String, String>,
    pub(super) next_wire: usize,
}

#[derive(Clone)]
pub(super) struct NodeRec {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) archetype: String,
    pub(super) title: String,
    pub(super) span: SourceSpan,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) badges: Vec<String>,
    pub(super) affordances: Vec<String>,
    pub(super) meta_json: Option<String>,
}

pub(super) struct PinRec {
    pub(super) id: String,
    pub(super) node_id: String,
    pub(super) name: String,
    pub(super) direction: String,
    pub(super) ty: String,
    pub(super) role: Option<String>,
    pub(super) pattern_source: Option<String>,
    pub(super) capability: String,
    pub(super) fallible: bool,
    pub(super) effect_grant_need: Option<String>,
    pub(super) span: SourceSpan,
    pub(super) pattern_source_span: Option<SourceSpan>,
    pub(super) append_op: Option<String>,
    pub(super) element_index: Option<usize>,
}

pub(super) struct WireRec {
    pub(super) id: String,
    pub(super) from_pin: String,
    pub(super) to_pin: String,
    pub(super) kind: String,
    pub(super) span: Option<SourceSpan>,
    pub(super) from_span: Option<SourceSpan>,
    pub(super) to_span: Option<SourceSpan>,
}

pub(super) struct InlineRec {
    pub(super) id: String,
    pub(super) node_id: String,
    pub(super) role: String,
    pub(super) source: String,
    pub(super) span: SourceSpan,
}

/// Stable source revision used by graph edit transactions.
pub fn source_revision(src: &str) -> String {
    format!("sha256-{}", SHA256::sha256_hex(src.as_bytes()))
}

/// Project a checked Jet file into the public Canvas graph schema.
pub fn graph_json_for_file(path: &Path) -> Result<String, Vec<Diagnostic>> {
    project_file(path).map(|p| p.json)
}

/// Project a file graph selected from the entry file's package/workspace graph.
pub fn graph_json_for_entry_source(
    entry: &Path,
    source_id: Option<&str>,
) -> Result<String, String> {
    let path = resolve_entry_source_path(entry, source_id)?;
    graph_json_for_file(&path).map_err(|diags| {
        let src = fs::read_to_string(&path).unwrap_or_default();
        query_error(
            "diagnostic",
            &jet_driver::Diagnostics::render_all(&path.display().to_string(), &src, &diags),
        )
    })
}

/// Project executed Event/AsyncEvent/DecisionHook facts from the existing
/// owner-checked, payload-free live snapshot onto a Canvas graph.
pub fn graph_json_for_entry_source_with_live_pid(
    entry: &Path,
    source_id: Option<&str>,
    pid: u32,
) -> Result<String, String> {
    let snapshot =
        crate::LiveInspect::read(pid).map_err(|message| query_error("live", &message))?;
    let runtime_events = runtime_events_json(&snapshot)?;
    let path = resolve_entry_source_path(entry, source_id)?;
    project_file_with_runtime(&path, Some(&runtime_events))
        .map(|projection| projection.json)
        .map_err(|diags| {
            let src = fs::read_to_string(&path).unwrap_or_default();
            query_error(
                "diagnostic",
                &jet_driver::Diagnostics::render_all(&path.display().to_string(), &src, &diags),
            )
        })
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
    let wanted = clean_project_rel_path(source_id).ok()?;
    if Path::new(&wanted).extension().and_then(|e| e.to_str()) != Some(jet_driver::Syntax::FILE_EXT) {
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
    vec![ctx.entry_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()]
}

/// Project package/workspace source truth into the public Canvas project schema.
pub fn project_json_for_entry(path: &Path) -> String {
    // Parent-walk discovery can parse `.jet` files; use the compiler stack +
    // TIR bridge rather than the thin test/UI thread (default ~2MiB).
    jet_driver::run_compiler_work(|| project_json_for_entry_inner(path))
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
                json_str(&part.name),
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
                json_str(&conflict.name),
                paths
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let locks_json = lock_project_json(&ctx.project_root);
    let env_projection = env_project_json(&ctx.project_root);
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
        "{{\"protocol\":\"jet.canvas.project\",\"schema_version\":{},\"project_root\":{},\"project_revision\":{},\"entry\":{},\"mode\":{},\"workspace\":{},\"packages\":[{}],\"targets\":[{}],\"envs\":[{}],\"services\":[{}],\"files\":[{}],\"parts\":[{}],\"part_conflicts\":[{}],\"locks\":[{}],\"diagnostics\":[{}],\"source_control\":{{\"truth\":\"git-text\"}},\"state_policy\":{{\"semantic\":\"source\",\"local\":[\"tabs\",\"viewport\",\"selection\",\"breakpoints\",\"watches\",\"comment_boxes\",\"staged_nodes\"],\"shared_visual\":\"source-anchored-comments\"}}}}",
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
        workspace_json,
        packages_json,
        targets_json,
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
    let ctx = project_context_for_entry(path);
    let schema = json_usize_field(request, "schema_version").unwrap_or(0);
    if schema != PROJECT_SCHEMA_VERSION as usize {
        return Err(project_edit_error(
            "schema",
            "Canvas project transaction schema_version must be 1",
        ));
    }
    let _project_revision = required_project_string(request, "project_revision")?;
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
        _ => Err(project_edit_error(
            "unsupported",
            "unknown Canvas project transaction operation",
        )),
    }
}

/// Query Canvas graph/source facts through the same semindex data LSP consumes.
pub fn query_json_for_file(path: &Path, request: &str) -> Result<String, String> {
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
    let source_id = json_string_field(request, "source_id");
    let path = resolve_entry_source_path(entry, source_id.as_deref())?;
    query_json_for_file(&path, request)
}

/// Expose the canonical Core library catalog to Canvas without granting
/// execution authority.
pub fn core_catalog_json_for_entry(entry: &Path, query: &str) -> Result<String, String> {
    let path = resolve_entry_source_path(entry, None)?;
    let src = fs::read_to_string(&path).map_err(|e| query_error("io", &e.to_string()))?;
    canvas_core_catalog(&path, &src, query)
}

/// Report Git text truth for Canvas source-control UI.
pub fn source_control_json_for_file(path: &Path) -> String {
    let src = fs::read_to_string(path).unwrap_or_default();
    let Some(root) = git_root(path) else {
        return format!(
            "{{\"protocol\":\"jet.canvas.source_control\",\"schema_version\":1,\"ok\":true,\"revision\":{},\"available\":false,\"dirty\":false,\"status\":{},\"diff\":{},\"history\":[]}}",
            json_str(&source_revision(&src)),
            json_str("not a Git worktree"),
            json_str("")
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
        "{{\"protocol\":\"jet.canvas.source_control\",\"schema_version\":1,\"ok\":true,\"revision\":{},\"available\":true,\"dirty\":{},\"status\":{},\"diff\":{},\"history\":[{}]}}",
        json_str(&source_revision(&src)),
        if dirty { "true" } else { "false" },
        json_str(status.trim()),
        json_str(&diff),
        history
    )
}

/// Report Git text truth for the whole projected package/workspace.
pub fn source_control_json_for_entry(path: &Path) -> String {
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
            format!(
                "{{\"path\":{},\"revision\":{},\"kind\":{},\"available\":{},\"dirty\":{},\"status\":{},\"diff\":{}}}",
                json_str(&file.path),
                json_str(&file.revision),
                json_str(&file.kind),
                if git_root.is_some() { "true" } else { "false" },
                if dirty { "true" } else { "false" },
                json_str(status.trim()),
                json_str(&diff)
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
    let path = resolve_entry_source_path(entry, source_id)?;
    let src = fs::read_to_string(&path).map_err(|e| query_error("io", &e.to_string()))?;
    let revision = source_revision(&src);
    let check = match project_file(&path) {
        Ok(_) => {
            "{\"state\":\"ok\",\"diagnostics_count\":0,\"message\":\"front end check passed\"}"
                .to_string()
        }
        Err(diags) => {
            let message = jet_driver::Diagnostics::render_all(&path.display().to_string(), &src, &diags);
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
    let budget_root = jet_driver::Loader::find_manifest_root(path.parent().unwrap_or(Path::new(".")))
        .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
    let budget_path = path.strip_prefix(&budget_root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
    let budget_digest = jet_driver::SHA256::sha256_hex(src.as_bytes());
    let budgets = jet_driver::BudgetView::read_compatible(&budget_root, &[(budget_path, budget_digest)]);
    let budget_reports = budgets.facts.iter().map(|fact| format!(
        "{{\"budget_id\":{},\"enforcement\":{},\"evidence\":{},\"evidence_id\":{},\"outcome\":{},\"report_id\":{},\"statistical\":{}}}",
        json_str(&fact.budget_id), json_str(&fact.enforcement), json_str(&fact.evidence), json_str(&fact.evidence_id),
        json_str(&fact.outcome), json_str(&fact.report_id), fact.statistical
    )).collect::<Vec<_>>().join(",");
    let budget_rejected = budgets.rejected.iter().map(|reason| json_str(reason)).collect::<Vec<_>>().join(",");
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
    let src = fs::read_to_string(entry).map_err(|e| query_error("io", &e.to_string()))?;
    let revision = required_string(request, "revision")?;
    if revision != source_revision(&src) {
        return Err(query_error(
            "conflict",
            "source changed since this Canvas command was approved",
        ));
    }
    let action_id = required_string(request, "action_id")?;
    let source = entry
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(jet_driver::Syntax::DEFAULT_ENTRY_FILE);
    let (label, args, writes, requires_confirmation) = match action_id.as_str() {
        "canvas.command:run" => ("Run program", vec!["run", source], "none", false),
        "canvas.command:check" => ("Check project", vec!["check", source], "none", false),
        "canvas.command:build" => ("Build project", vec!["build", source], "build_outputs", true),
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
        let abs = canonical_path(entry);
        let overlay = source_override.as_deref().map(|text| (abs.as_path(), text));
        let (diags, _bundle, _facts) =
            jet_driver::Driver::check_file_with_effect_facts(&entry.display().to_string(), overlay, true);
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
                jet_driver::Diagnostics::render_all(&entry.display().to_string(), check_src, &errors),
                diagnostics_json(entry, check_src, &errors),
            )
        }
    } else {
        let output = run_jet_command(entry, &args)?;
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
        "{{\"protocol\":\"jet.canvas.command_receipt\",\"schema_version\":1,\"ok\":true,\"action_id\":{},\"title\":{},\"revision\":{},\"checked_revision\":{},\"command\":[{}],\"writes\":{},\"success\":{},\"exit_code\":{},\"elapsed_ms\":{},\"stdout\":{},\"stderr\":{},\"diagnostics\":[{}]}}",
        json_str(&action_id),
        json_str(label),
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
    jet_driver::run_compiler_work(|| apply_transaction_json_on_compiler_stack(path, request))
}

fn apply_transaction_json_on_compiler_stack(path: &Path, request: &str) -> Result<String, String> {
    let src = fs::read_to_string(path).map_err(|e| edit_error("io", &e.to_string()))?;
    let revision = required_string(request, "revision")?;
    if revision != source_revision(&src) {
        return Err(edit_error(
            "conflict",
            "source changed since this Canvas graph was drawn",
        ));
    }
    let schema = json_usize_field(request, "schema_version").unwrap_or(0);
    if schema != EDIT_SCHEMA_VERSION as usize {
        return Err(edit_error("schema", "Canvas edit schema_version must be 1"));
    }
    let op = required_string(request, "op")?;
    match op.as_str() {
        "noop" => apply_noop(path, &src),
        "rename_binding" => {
            let from = required_string(request, "from")?;
            let to = required_string(request, "to")?;
            validate_ident(&from)?;
            validate_ident(&to)?;
            apply_rename(path, &src, &from, &to)
        }
        "rename_function" => {
            let from = required_string(request, "from")?;
            let to = required_string(request, "to")?;
            validate_ident(&from)?;
            validate_ident(&to)?;
            apply_rename(path, &src, &from, &to)
        }
        "create_function" => {
            let name = required_string(request, "name")?;
            validate_ident(&name)?;
            let params = json_string_field(request, "params").unwrap_or_default();
            validate_signature_fragment(&params)?;
            let ret_type =
                json_string_field(request, "ret_type").unwrap_or_else(|| "Int".to_string());
            validate_type_fragment(&ret_type)?;
            apply_create_function(path, &src, &name, &params, &ret_type)
        }
        "edit_function_signature" => {
            let graph_id = required_string(request, "graph_id")?;
            let signature = required_string(request, "signature")?;
            validate_function_signature(&signature)?;
            apply_edit_function_signature(path, &src, &graph_id, &signature)
        }
        "edit_inline_expr" => {
            let inline_id = required_string(request, "inline_expr_id")?;
            let new_expr = required_string(request, "new_expr")?;
            apply_inline_edit(path, &src, &inline_id, &new_expr)
        }
        "promote_to_binding" => {
            let inline_id = required_string(request, "inline_expr_id")?;
            let name = required_string(request, "name")?;
            validate_ident(&name)?;
            apply_promote_inline(path, &src, &inline_id, &name)
        }
        "insert_visible_conversion" => {
            let inline_id = required_string(request, "inline_expr_id")?;
            let callee = required_string(request, "callee")?;
            validate_qualified_name(&callee)?;
            apply_visible_conversion(path, &src, &inline_id, &callee)
        }
        "break_link" => {
            let wire_id = required_string(request, "wire_id")?;
            apply_break_link(path, &src, &wire_id)
        }
        "move_link" => {
            let wire_id = required_string(request, "wire_id")?;
            let replacement = required_string(request, "replacement")?;
            validate_qualified_name(&replacement)?;
            apply_move_link(path, &src, &wire_id, &replacement)
        }
        "reorder_statements" => {
            let graph_id = required_string(request, "graph_id")?;
            let moved = SourceSpan {
                start: json_usize_field(request, "moved_start").ok_or_else(|| {
                    edit_error("bad_request", "missing `moved_start`")
                })?,
                end: json_usize_field(request, "moved_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `moved_end`"))?,
            };
            let anchor = SourceSpan {
                start: json_usize_field(request, "anchor_start").ok_or_else(|| {
                    edit_error("bad_request", "missing `anchor_start`")
                })?,
                end: json_usize_field(request, "anchor_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `anchor_end`"))?,
            };
            let position = json_string_field(request, "position").unwrap_or_else(|| "after".into());
            apply_reorder_statements(path, &src, &graph_id, moved, anchor, &position)
        }
        "add_pattern_arm" => {
            let graph_id = required_string(request, "graph_id")?;
            let pattern = required_string(request, "pattern")?;
            validate_single_line_fragment(&pattern, "pattern arm text must stay on one line")?;
            let node = SourceSpan {
                start: json_usize_field(request, "node_start")
                    .ok_or_else(|| edit_error("bad_request", "missing `node_start`"))?,
                end: json_usize_field(request, "node_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `node_end`"))?,
            };
            apply_add_pattern_arm(path, &src, &graph_id, node, &pattern)
        }
        "edit_pattern_arm" => {
            let graph_id = required_string(request, "graph_id")?;
            let pattern = required_string(request, "pattern")?;
            validate_single_line_fragment(&pattern, "pattern arm text must stay on one line")?;
            let pattern_span = SourceSpan {
                start: json_usize_field(request, "pattern_start")
                    .ok_or_else(|| edit_error("bad_request", "missing `pattern_start`"))?,
                end: json_usize_field(request, "pattern_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `pattern_end`"))?,
            };
            apply_edit_pattern_arm(path, &src, &graph_id, pattern_span, &pattern)
        }
        "remove_pattern_arm" => {
            let graph_id = required_string(request, "graph_id")?;
            let pattern_span = SourceSpan {
                start: json_usize_field(request, "pattern_start")
                    .ok_or_else(|| edit_error("bad_request", "missing `pattern_start`"))?,
                end: json_usize_field(request, "pattern_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `pattern_end`"))?,
            };
            apply_remove_pattern_arm(path, &src, &graph_id, pattern_span)
        }
        "append_multi_input" => {
            let element = json_string_field(request, "element").unwrap_or_else(|| "1".to_string());
            validate_single_line_fragment(&element, "multi-input element must stay on one line")?;
            let node = SourceSpan {
                start: json_usize_field(request, "node_start")
                    .ok_or_else(|| edit_error("bad_request", "missing `node_start`"))?,
                end: json_usize_field(request, "node_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `node_end`"))?,
            };
            apply_append_multi_input(path, &src, node, &element)
        }
        "remove_multi_input_element" => {
            let node = SourceSpan {
                start: json_usize_field(request, "node_start")
                    .ok_or_else(|| edit_error("bad_request", "missing `node_start`"))?,
                end: json_usize_field(request, "node_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `node_end`"))?,
            };
            let element = SourceSpan {
                start: json_usize_field(request, "element_start")
                    .ok_or_else(|| edit_error("bad_request", "missing `element_start`"))?,
                end: json_usize_field(request, "element_end")
                    .ok_or_else(|| edit_error("bad_request", "missing `element_end`"))?,
            };
            apply_remove_multi_input_element(path, &src, node, element)
        }
        "replace_source" => {
            let source = required_string(request, "source")?;
            if json_string_field(request, "undo_restore").is_some() {
                write_checked_source(path, &src, &source)
            } else {
                write_checked_formatted(path, &src, &source)
            }
        }
        "insert_branch" | "insert_switch" | "insert_loop" | "insert_fallible_rail" => {
            let graph_id = required_string(request, "graph_id")?;
            apply_insert_structural(path, &src, &graph_id, op.as_str())
        }
        "create_comment_region" => {
            let graph_id = required_string(request, "graph_id")?;
            let title = required_string(request, "title")?;
            let color =
                json_string_field(request, "color").unwrap_or_else(|| "#2563eb".to_string());
            let alpha = json_string_field(request, "alpha").unwrap_or_else(|| "0.18".to_string());
            let start = json_usize_field(request, "start").unwrap_or(0);
            let end = json_usize_field(request, "end").unwrap_or(start);
            let bounds =
                json_string_field(request, "bounds").unwrap_or_else(|| "0,0,360,180".to_string());
            apply_create_comment_region(
                path, &src, &graph_id, start, end, &title, &color, &alpha, &bounds,
            )
        }
        "edit_comment_region" | "move_comment_region" | "resize_comment_region" => {
            let region_id = required_string(request, "region_id")?;
            let title = json_string_field(request, "title");
            let color = json_string_field(request, "color");
            let alpha = json_string_field(request, "alpha");
            let bounds = json_string_field(request, "bounds");
            apply_update_comment_region(
                path,
                &src,
                &region_id,
                title.as_deref(),
                color.as_deref(),
                alpha.as_deref(),
                bounds.as_deref(),
            )
        }
        "delete_comment_region" => {
            let region_id = required_string(request, "region_id")?;
            apply_delete_comment_region(path, &src, &region_id)
        }
        "create_collapsed_region" => {
            let graph_id = required_string(request, "graph_id")?;
            let title = required_string(request, "title")?;
            let start = json_usize_field(request, "start").unwrap_or(0);
            let end = json_usize_field(request, "end").unwrap_or(start);
            apply_create_collapse_region(path, &src, &graph_id, start, end, &title)
        }
        "expand_collapsed_region" => {
            let region_id = required_string(request, "region_id")?;
            apply_delete_hint_region(path, &src, &region_id, "collapse")
        }
        "extract_inline_expr" | "preview_extract_inline_expr" => {
            let inline_id = required_string(request, "inline_expr_id")?;
            let function = required_string(request, "function")?;
            validate_ident(&function)?;
            let ret_type =
                json_string_field(request, "ret_type").unwrap_or_else(|| "Int".to_string());
            let candidate = extract_inline_candidate(path, &src, &inline_id, &function, &ret_type)?;
            if op == "preview_extract_inline_expr" {
                Ok(preview_ok(&src, &candidate))
            } else {
                write_checked_formatted(path, &src, &candidate)
            }
        }
        "inline_helper_call" => {
            let inline_id = json_string_field(request, "inline_expr_id");
            let start = json_usize_field(request, "start");
            let end = json_usize_field(request, "end");
            let candidate = inline_helper_candidate(path, &src, inline_id.as_deref(), start, end)?;
            write_checked_formatted(path, &src, &candidate)
        }
        "insert_call" => {
            let graph_id = required_string(request, "graph_id")?;
            let callee = required_string(request, "callee")?;
            validate_qualified_name(&callee)?;
            let args = json_string_array(request, "args");
            let bind = json_string_field(request, "bind");
            let wire_inline_expr_id = json_string_field(request, "wire_inline_expr_id");
            let wire_expr = json_string_field(request, "wire_expr");
            if let Some(name) = &bind {
                validate_ident(name)?;
            }
            if let Some(expr) = &wire_expr {
                validate_qualified_name(expr)?;
            }
            apply_insert_call(
                path,
                &src,
                &graph_id,
                &callee,
                &args,
                bind.as_deref(),
                wire_inline_expr_id.as_deref(),
                wire_expr.as_deref(),
            )
        }
        "create_trait_impl" => {
            let type_name = required_string(request, "type_name")?;
            let trait_name = required_string(request, "trait_name")?;
            validate_qualified_name(&type_name)?;
            validate_qualified_name(&trait_name)?;
            apply_create_trait_impl(path, &src, &type_name, &trait_name)
        }
        "preview_canvas_action" => {
            let graph_id = required_string(request, "graph_id")?;
            let action_id = required_string(request, "action_id")?;
            let callee = required_string(request, "callee")?;
            validate_qualified_name(&callee)?;
            let args = json_string_array(request, "args");
            let candidate =
                canvas_action_candidate(path, &src, &graph_id, &action_id, &callee, &args)?;
            Ok(canvas_action_preview_ok(
                path, &src, &candidate, &action_id, &callee,
            ))
        }
        _ => Err(edit_error("unsupported", "unknown Canvas edit operation")),
    }
}

/// Run one source-level debugger slice and project it onto Canvas graph spans.
pub fn debug_session_json_for_file(path: &Path, request: &str) -> Result<String, String> {
    let src = fs::read_to_string(path).map_err(|e| debug_error("io", &e.to_string()))?;
    let revision = required_debug_string(request, "revision")?;
    if revision != source_revision(&src) {
        return Err(debug_error(
            "conflict",
            "source changed since this Canvas debug state was drawn",
        ));
    }
    let schema = json_usize_field(request, "schema_version").unwrap_or(0);
    if schema != DEBUG_SCHEMA_VERSION as usize {
        return Err(debug_error(
            "schema",
            "Canvas debug schema_version must be 1",
        ));
    }

    let mut inputs = Vec::new();
    let mut breakpoint_lines = json_usize_array(request, "breakpoints");
    for anchor in json_string_array(request, "breakpoint_spans") {
        if let Some(line) = line_from_anchor(&src, &anchor) {
            breakpoint_lines.push(line);
        }
    }
    breakpoint_lines.sort_unstable();
    breakpoint_lines.dedup();
    for line in &breakpoint_lines {
        inputs.push(format!("break {line}"));
    }

    let mut commands = json_string_array(request, "commands");
    if commands.is_empty() {
        commands.push("s".to_string());
    }
    inputs.extend(commands);
    inputs.push("locals".to_string());
    let watches = json_string_array(request, "watches");
    for watch in &watches {
        inputs.push(format!("p {watch}"));
    }
    inputs.push("bt".to_string());
    let refs = inputs.iter().map(|s| s.as_str()).collect::<Vec<_>>();
    let result = jet_debug::run_session_result(&path.display().to_string(), &refs);
    if result.status == jet_debug::SessionStatus::Failed {
        return Err(debug_error("diagnostic", &result.transcript));
    }

    let projection =
        project_file(path).map_err(|diags| debug_diagnostics_error(path, &src, &diags))?;
    Ok(debug_ok(
        &src,
        &projection.json,
        &result.transcript,
        result.status,
        &breakpoint_lines,
        &watches,
    ))
}
