//! D-BPE-* Canvas: source-backed graph projection and v1 edit transactions.
//!
//! Canvas is a client of the checked front end. It does not parse or type-check
//! by a second path: projection comes from `ProgramBundle` + semindex facts, and
//! writes go back through `jet fmt` before the file is replaced.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Diagnostics::{Diagnostic, Severity, Span, TextEdit};
use crate::AST::{self, Expr, Item, Stmt};
use crate::{FixEngine, SHA256};
use jet_semindex::{SemIndex, SemIndexEffectFacts, SourceSpan, SymbolKind};

pub const GRAPH_SCHEMA_VERSION: u32 = 1;
pub const EDIT_SCHEMA_VERSION: u32 = 1;
pub const DEBUG_SCHEMA_VERSION: u32 = 1;
pub const QUERY_SCHEMA_VERSION: u32 = 1;
pub const ACTION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
struct InlineExpr {
    id: String,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct GraphEditAnchor {
    graph_id: String,
    insert_offset: usize,
}

#[derive(Debug, Clone)]
struct NodeQueryRef {
    graph_id: String,
    node_id: String,
    kind: String,
    title: String,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct Projection {
    json: String,
    inline_exprs: Vec<InlineExpr>,
    graph_anchors: Vec<GraphEditAnchor>,
    node_refs: Vec<NodeQueryRef>,
}

#[derive(Default)]
struct GraphBuilder {
    graph_id: String,
    nodes: Vec<NodeRec>,
    pins: Vec<PinRec>,
    wires: Vec<WireRec>,
    regions: Vec<String>,
    inline_exprs: Vec<InlineRec>,
    local_pins: HashMap<String, String>,
    local_types: HashMap<String, String>,
    next_wire: usize,
}

#[derive(Clone)]
struct NodeRec {
    id: String,
    kind: String,
    title: String,
    span: SourceSpan,
    x: i32,
    y: i32,
    badges: Vec<String>,
    affordances: Vec<String>,
}

struct PinRec {
    id: String,
    node_id: String,
    name: String,
    direction: String,
    ty: String,
    capability: String,
    fallible: bool,
    effect_grant_need: Option<String>,
    span: SourceSpan,
}

struct WireRec {
    id: String,
    from_pin: String,
    to_pin: String,
    kind: String,
    span: Option<SourceSpan>,
}

struct InlineRec {
    id: String,
    node_id: String,
    role: String,
    source: String,
    span: SourceSpan,
}

/// Stable source revision used by graph edit transactions.
pub fn source_revision(src: &str) -> String {
    format!("sha256-{}", SHA256::sha256_hex(src.as_bytes()))
}

/// Project a checked Jet file into the public Canvas graph schema.
pub fn graph_json_for_file(path: &Path) -> Result<String, Vec<Diagnostic>> {
    project_file(path).map(|p| p.json)
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
        _ => Err(query_error("unsupported", "unknown Canvas query operation")),
    }
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

/// Apply one versioned Canvas edit transaction and write ordinary Jet source.
pub fn apply_transaction_json(path: &Path, request: &str) -> Result<String, String> {
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
        "replace_source" => {
            let source = required_string(request, "source")?;
            write_checked_formatted(path, &src, &source)
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
            if let Some(name) = &bind {
                validate_ident(name)?;
            }
            apply_insert_call(path, &src, &graph_id, &callee, &args, bind.as_deref())
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
                &src, &candidate, &action_id, &callee,
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
    let transcript = crate::Debug::run_session(&path.display().to_string(), &refs);
    if transcript.contains("Error [") || transcript.contains("\n[E") || transcript.starts_with("[E")
    {
        return Err(debug_error("diagnostic", &transcript));
    }

    let projection =
        project_file(path).map_err(|diags| debug_diagnostics_error(path, &src, &diags))?;
    Ok(debug_ok(
        &src,
        &projection.json,
        &transcript,
        &breakpoint_lines,
        &watches,
    ))
}

pub fn canvas_html() -> String {
    canvas_html_for("/canvas")
}

pub fn canvas_html_for(base: &str) -> String {
    canvas_html_document(&format!(
        r#"<script>window.__JET_CANVAS_BASE__ = "{}";</script>
<script src="{}/app.js?canvas_ui=blueprint23"></script>"#,
        json_escape(base),
        json_escape(base)
    ))
}

pub fn canvas_html_query() -> String {
    canvas_html_document(
        r#"<script>window.__JET_CANVAS_BASE__ = ""; window.__JET_CANVAS_GRAPH__ = "/?jet_panel_graph=1"; window.__JET_CANVAS_TX__ = "/canvas/transaction"; window.__JET_CANVAS_QUERY__ = "/canvas/query"; window.__JET_CANVAS_SCM__ = "/canvas/source-control";</script>
<script src="/?jet_panel_app=1&canvas_ui=blueprint23"></script>"#,
    )
}

fn canvas_html_document(bootstrap: &str) -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Jet Canvas</title>
<style>
* { box-sizing: border-box; }
html, body { margin: 0; height: 100%; overflow: hidden; background: #05070b; color: #d7e4f7; font: 13px "Inter", "Segoe UI", system-ui, sans-serif; }
body:not(.is-dev-mode) .dev-only { display: none !important; }
button, input, select { font: inherit; }
button { color: #d7e4f7; border: 1px solid #31445d; background: linear-gradient(#18202b, #111821); min-height: 30px; padding: 0 10px; cursor: pointer; border-radius: 4px; box-shadow: inset 0 1px 0 rgba(255,255,255,.04), 0 1px 0 rgba(0,0,0,.4); white-space: nowrap; }
button:hover, button:focus-visible { border-color: #35c2ff; background: #1d2b3a; outline: none; box-shadow: 0 0 0 2px rgba(53,194,255,.18); }
button.primary { background: #123d45; border-color: #22d3ee; color: #e7fbff; }
button.is-active { border-color: #f6d365; background: #352c17; color: #fff4bd; }
button:disabled { opacity: .42; cursor: default; box-shadow: none; }
input, select { color: #e7eefb; border: 1px solid #31445d; background: #0b1118; min-height: 30px; padding: 0 8px; border-radius: 4px; }
input:focus-visible, select:focus-visible { border-color: #35c2ff; outline: none; box-shadow: 0 0 0 2px rgba(53,194,255,.18); }
select { min-width: 180px; }
#shell { height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr) 28px; min-width: 0; }
#topbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; min-width: 0; min-height: 54px; padding: 8px 10px; border-bottom: 1px solid #25364b; background: linear-gradient(#111923, #0c1119 62%, #080c12); box-shadow: 0 1px 0 rgba(255,255,255,.04) inset, 0 14px 40px rgba(0,0,0,.25); }
#brand { display: flex; flex-direction: column; gap: 1px; flex: 0 1 164px; min-width: 124px; padding-left: 8px; border-left: 3px solid #35c2ff; }
#brand strong { font-size: 14px; letter-spacing: .08em; text-transform: uppercase; color: #f8fbff; }
#brand span { color: #9db4d2; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#graph-select { display: none; border-color: #4b6685; background: #0c1420; color: #eaf5ff; }
body.is-dev-mode #graph-select { display: block; }
#topbar > select { flex: 1 1 190px; min-width: 140px; max-width: 360px; }
#topbar > button { flex: 0 0 auto; padding-inline: 8px; }
#jump { flex: 1 1 150px; min-width: 94px; color: #9db4d2; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
body:not(.is-dev-mode) #jump { display: none; }
.debug-controls { margin-left: auto; display: flex; align-items: center; gap: 5px; flex: 1 1 380px; min-width: 0; justify-content: flex-end; flex-wrap: wrap; }
body:not(.is-dev-mode) .debug-controls { display: none; }
.debug-controls select { flex: 1 1 130px; min-width: 112px; max-width: 220px; }
.debug-controls button { min-width: 30px; padding: 0 7px; }
#workbench { min-height: 0; min-width: 0; position: relative; display: grid; grid-template-columns: minmax(156px, 15vw) minmax(0, 1fr) minmax(238px, 20vw); background: #05070b; }
.side { min-width: 0; overflow: hidden auto; background: #0b1017; border-right: 1px solid #23344a; box-shadow: inset -1px 0 0 rgba(255,255,255,.03); }
.right { border-right: 0; border-left: 1px solid #23344a; box-shadow: inset 1px 0 0 rgba(255,255,255,.03); }
.panel { border-bottom: 1px solid #23344a; padding: clamp(9px, 1.2vw, 13px); }
.panel h2 { margin: 0 0 10px; color: #eaf5ff; font-size: 11px; letter-spacing: .12em; text-transform: uppercase; }
.panel details { display: grid; gap: 8px; }
.panel summary { list-style: none; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 8px; min-height: 30px; color: #eaf5ff; font-size: 11px; letter-spacing: .12em; text-transform: uppercase; cursor: pointer; }
.panel summary::-webkit-details-marker { display: none; }
.panel summary::before { content: "▸"; color: #35c2ff; font-size: 12px; transform: rotate(0deg); transition: transform .12s ease; }
.panel details[open] summary::before { transform: rotate(90deg); }
.panel summary .count { justify-self: end; }
.panel details > :not(summary) { margin-top: 8px; }
.graph-list, .search-results { display: grid; gap: 6px; }
.graph-item { width: 100%; display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 8px; text-align: left; border-color: #283b52; background: #101821; min-height: 38px; }
.graph-item.is-active { border-color: #35c2ff; background: #102437; box-shadow: inset 3px 0 0 #35c2ff; }
.search-item { width: 100%; text-align: left; border-color: #34285e; background: #151229; }
.search-item.is-active { border-color: #d58cff; background: #21133a; }
.search-item small { color: #9aa8c5; display: block; margin-top: 3px; overflow-wrap: anywhere; }
.count, .tag { color: #8fb2dc; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.search { width: 100%; margin-bottom: 8px; }
#stage { position: relative; min-width: 0; min-height: 0; overflow: hidden; background: #05070b; }
#jet-canvas-view { width: 100%; height: 100%; display: block; background: #05070b; }
#canvas-dock { position: absolute; left: 10px; top: 10px; z-index: 26; display: none; gap: 6px; padding: 5px; border: 1px solid #365a7f; background: rgba(8,17,29,.92); box-shadow: 0 12px 34px rgba(0,0,0,.42); border-radius: 6px; }
#canvas-dock button { min-height: 28px; padding: 0 8px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#canvas-dock button.is-active { border-color: #35c2ff; color: #e7fbff; background: #102437; }
#graph-strip { position: absolute; left: 12px; right: 12px; top: 10px; z-index: 13; display: flex; gap: 8px; overflow-x: auto; scrollbar-width: thin; padding: 6px; border: 1px solid rgba(53,194,255,.45); background: linear-gradient(180deg, rgba(9,19,32,.94), rgba(5,11,19,.82)); box-shadow: 0 16px 44px rgba(0,0,0,.42), inset 0 1px 0 rgba(255,255,255,.05); border-radius: 6px; max-width: min(860px, calc(100% - 24px)); }
body:not(.is-dev-mode) #graph-strip { display: none; }
.graph-tab { position: relative; flex: 0 0 auto; min-width: 116px; min-height: 36px; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 7px; align-items: center; border-color: #2e4966; background: linear-gradient(180deg, rgba(18,31,46,.96), rgba(8,17,29,.96)); font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; box-shadow: inset 0 -1px 0 rgba(255,255,255,.04); }
.graph-tab::before { content: ""; width: 8px; height: 18px; border-radius: 2px; background: #46617d; box-shadow: 0 0 14px rgba(70,97,125,.32); }
.graph-tab.is-active { border-color: #35c2ff; background: linear-gradient(180deg, #12324a, #0b1d2d); color: #eaf8ff; box-shadow: inset 0 -3px 0 #35c2ff, 0 0 28px rgba(53,194,255,.24); }
.graph-tab.is-active::before { background: #35c2ff; box-shadow: 0 0 18px rgba(53,194,255,.72); }
.graph-tab-title { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-weight: 700; color: #f2f8ff; }
.graph-tab-kind { color: #77a7d7; font-size: 9px; letter-spacing: .12em; text-transform: uppercase; }
.graph-tab-count { color: #8fb2dc; border: 1px solid #2b4a67; background: rgba(2,8,15,.42); border-radius: 999px; padding: 2px 6px; }
#wire-status { position: absolute; left: 10px; top: 58px; z-index: 13; display: grid; grid-template-columns: auto auto minmax(0, 1fr); align-items: center; gap: 7px; max-width: min(470px, calc(100% - 24px)); min-height: 34px; padding: 7px 10px; border: 1px solid rgba(54,90,127,.78); background: rgba(7,16,28,.82); box-shadow: 0 14px 34px rgba(0,0,0,.34); border-radius: 6px; color: #93b3d7; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; pointer-events: none; }
body:not(.is-dev-mode) #wire-status { display: none; }
#wire-status-dot { width: 9px; height: 9px; border-radius: 50%; background: var(--wire-color, #7dd3fc); box-shadow: 0 0 16px var(--wire-color, #7dd3fc); }
#wire-status b { color: #eaf5ff; font-weight: 700; }
#wire-status span:last-child { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
#graph-overview { display: none; }
.graph-overview-title { display: grid; gap: 2px; min-width: 0; }
.graph-overview-title b { color: #f8fbff; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.graph-overview-title code { color: #fde68a; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.graph-stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 6px; }
.graph-stat { display: grid; gap: 2px; padding: 6px; border: 1px solid #243852; background: rgba(8,16,27,.74); border-radius: 4px; min-width: 0; }
.graph-stat b { color: #eaf5ff; font-size: 13px; }
.graph-stat span { color: #84a8cf; font: 9px ui-monospace, "SFMono-Regular", Consolas, monospace; text-transform: uppercase; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
#source-view { position: absolute; inset: 0; display: none; margin: 0; padding: 20px 24px 84px; overflow: auto; color: #dbeafe; background: #07101a; border: 0; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; line-height: 1.6; white-space: pre; tab-size: 4; }
#stage.is-source #jet-canvas-view, #stage.is-source #minimap, #stage.is-source #graph-strip, #stage.is-source #wire-status, #stage.is-source #graph-overview { display: none; }
#stage.is-source #source-view { display: block; }
#minimap { position: absolute; right: clamp(8px, 1.4vw, 16px); bottom: clamp(8px, 1.4vw, 16px); width: min(210px, 22vw); height: min(132px, 15vw); min-width: 120px; min-height: 78px; border: 1px solid #365a7f; background: rgba(7,16,28,.9); box-shadow: 0 14px 42px rgba(0,0,0,.42); border-radius: 6px; }
#hud { position: absolute; left: clamp(8px, 1.4vw, 16px); bottom: clamp(8px, 1.4vw, 16px); display: flex; gap: 8px; color: #9bb4d3; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: calc(100% - 32px); flex-wrap: wrap; }
#hud span { border: 1px solid #263b59; background: rgba(8,17,29,.88); padding: 5px 8px; border-radius: 4px; }
body:not(.is-dev-mode) #graph-meta { display: none; }
#details { height: 100%; overflow: auto; --node-accent: #35c2ff; }
.details-empty { display: grid; gap: 8px; padding: 12px; border: 1px dashed #31445d; background: #0d1520; color: #90a5c4; border-radius: 4px; }
.details-hero { position: relative; display: grid; gap: 10px; padding: 12px; border: 1px solid color-mix(in srgb, var(--node-accent) 56%, #21334a); background: linear-gradient(180deg, color-mix(in srgb, var(--node-accent) 18%, #101926), #09111a); border-radius: 6px; box-shadow: inset 4px 0 0 var(--node-accent), 0 14px 36px rgba(0,0,0,.26); }
.details-titleline { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 9px; align-items: center; }
.node-glyph { display: grid; place-items: center; width: 36px; height: 30px; color: var(--node-accent); border: 1px solid color-mix(in srgb, var(--node-accent) 64%, #1d2d42); background: color-mix(in srgb, var(--node-accent) 13%, #08111b); border-radius: 4px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; box-shadow: 0 0 18px color-mix(in srgb, var(--node-accent) 28%, transparent); }
.details-title { min-width: 0; }
#details .title { color: #f8fbff; font-size: 17px; font-weight: 700; margin: 0 0 2px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
#details .kind { color: var(--node-accent); font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; text-transform: uppercase; letter-spacing: .08em; }
.details-chips { display: flex; flex-wrap: wrap; gap: 6px; }
.details-chip { color: #bfd4ee; border: 1px solid #2b405a; background: rgba(6,14,24,.56); border-radius: 999px; padding: 3px 7px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.quick-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 7px; }
.quick-actions button { min-width: 0; }
.quick-actions .wide { grid-column: 1 / -1; }
#details dl { display: grid; grid-template-columns: 72px 1fr; gap: 6px 9px; margin: 10px 0 0; padding-top: 9px; border-top: 1px solid #21344b; }
#details dt { color: #8096b5; font-size: 11px; }
#details dd { margin: 0; color: #d8e7fb; overflow-wrap: anywhere; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.pin-list, .inline-list { display: grid; gap: 7px; margin-top: 8px; }
.pin-row, .inline-row { border: 1px solid #2d4056; background: #101821; padding: 8px; border-radius: 4px; }
.pin-row { display: grid; gap: 6px; }
.pin-row b, .inline-row b { color: #f2f7ff; }
.pin-card { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 8px; border: 1px solid color-mix(in srgb, var(--pin-color) 42%, #253852); background: linear-gradient(180deg, color-mix(in srgb, var(--pin-color) 10%, #101821), #0a121b); padding: 8px; border-radius: 4px; box-shadow: inset 3px 0 0 var(--pin-color); }
.pin-card-title { min-width: 0; }
.pin-card-title b { display: block; color: #f2f7ff; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pin-card-title small { display: block; margin-top: 2px; color: #91a7c4; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pin-card button { min-height: 26px; padding-inline: 8px; }
.inline-row code { display: block; color: #fde68a; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; margin-top: 4px; white-space: pre-wrap; }
.edit-grid { display: grid; gap: 8px; margin-top: 10px; }
.edit-grid label { display: grid; gap: 4px; color: #90a5c4; }
.signature-board { display: grid; gap: 10px; margin: 10px 0 14px; padding: 10px; border: 1px solid #385a7e; background: linear-gradient(180deg, #0d1825, #09111b); border-radius: 6px; box-shadow: inset 0 1px 0 rgba(255,255,255,.04); }
.signature-head { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: 8px; padding-bottom: 8px; border-bottom: 1px solid #223954; }
.signature-head b { display: block; color: #f8fbff; font-size: 14px; overflow-wrap: anywhere; }
.signature-head code, .signature-source code { display: block; color: #fde68a; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; line-height: 1.45; white-space: pre-wrap; overflow-wrap: anywhere; }
.sig-eyebrow, .lane-meta { color: #84a8cf; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .09em; text-transform: uppercase; }
.signature-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 7px; }
.signature-actions .primary { grid-column: 1 / -1; }
.signature-source { display: grid; gap: 7px; padding: 8px; border: 1px solid #263c55; background: #0a121c; border-radius: 4px; }
.signature-source input { width: 100%; }
.rename-strip { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 7px; }
.pin-lane { display: grid; gap: 7px; padding: 8px; border: 1px solid #243852; background: rgba(8,16,27,.78); border-radius: 4px; }
.lane-head { display: flex; align-items: center; gap: 8px; min-width: 0; flex-wrap: wrap; }
.lane-head b { color: #eaf5ff; letter-spacing: .06em; text-transform: uppercase; font-size: 11px; }
.lane-head .lane-meta { margin-left: auto; white-space: nowrap; }
.lane-head button { min-height: 26px; padding-inline: 8px; flex: 0 0 auto; }
.pin-editor-row { display: grid; gap: 7px; padding: 8px; border: 1px solid #2c4868; background: #0e1722; border-radius: 4px; }
.pin-editor-title { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; min-width: 0; }
.pin-editor-title b { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pin-port { width: 12px; height: 12px; border-radius: 50%; display: inline-block; box-shadow: 0 0 0 3px rgba(255,255,255,.07), 0 0 16px currentColor; background: currentColor; }
.type-chip { color: #cfe9ff; border: 1px solid currentColor; background: rgba(255,255,255,.04); border-radius: 999px; padding: 2px 7px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: 98px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pin-empty { color: #7890ad; border: 1px dashed #31445d; padding: 8px; border-radius: 4px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.pin-tools { display: grid; grid-template-columns: minmax(0, 1fr) 68px 32px; gap: 6px; margin-top: 2px; }
.pin-tools.output-pin-tools { grid-template-columns: minmax(0, 1fr) 68px 46px; }
.pin-tools [data-param-default] { grid-column: 1 / 3; }
.pin-tools input { min-width: 0; }
#context-menu { position: fixed; z-index: 30; display: none; min-width: 320px; max-width: min(430px, calc(100vw - 20px)); border: 1px solid #3b5f89; background: #091525; box-shadow: 0 18px 48px rgba(0,0,0,.55); padding: 8px; border-radius: 6px; }
#context-menu.is-open { display: grid; gap: 6px; }
#context-menu button { width: 100%; text-align: left; display: grid; grid-template-columns: 1fr auto; gap: 10px; }
#context-menu .menu-title { color: #dbeafe; font-size: 11px; letter-spacing: .1em; text-transform: uppercase; padding: 4px 6px; }
.action-palette-head { display: grid; gap: 6px; padding: 4px; border-bottom: 1px solid #203954; }
.action-palette-head input { width: 100%; border-color: #3b5f89; background: #07111f; }
.action-context { color: #8fb2dc; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; display: flex; gap: 7px; align-items: center; min-width: 0; }
.action-context .pin-port { width: 9px; height: 9px; box-shadow: 0 0 0 2px rgba(255,255,255,.07), 0 0 12px currentColor; }
.action-results { display: grid; gap: 5px; max-height: min(360px, calc(100vh - 170px)); overflow: auto; padding: 2px; }
.action-result { border-color: #284866; background: #0d1826; min-height: 44px; }
.action-result:hover, .action-result:focus-visible { border-color: #35c2ff; background: #102942; }
.action-result small { color: #9aaecb; display: block; margin-top: 2px; overflow-wrap: anywhere; }
.action-empty { color: #8da4c2; padding: 9px; border: 1px dashed #31445d; border-radius: 4px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#statusbar { display: flex; align-items: center; gap: 14px; padding: 0 12px; border-top: 1px solid #23344a; background: #070b10; color: #8096b5; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#toast { margin-left: auto; color: #a7f3d0; }
@media (max-width: 1120px) {
  #workbench { grid-template-columns: minmax(142px, 18vw) minmax(0, 1fr) minmax(200px, 23vw); }
  #brand span { display: none; }
  #topbar { gap: 6px; }
  .debug-controls { flex-basis: 280px; }
  .debug-controls button, #topbar > button { padding-inline: 6px; }
}
@media (max-width: 900px) {
  #workbench { grid-template-columns: minmax(0, 1fr); }
  #canvas-dock { display: flex; }
  #graph-strip { top: 56px; left: 10px; right: 10px; max-width: calc(100% - 20px); }
  #wire-status { top: 104px; left: 10px; right: 10px; max-width: none; }
  #graph-overview { display: none; }
  .side { display: none; position: absolute; top: 0; bottom: 0; left: 0; width: min(326px, calc(100vw - 54px)); z-index: 22; border-right: 1px solid #35c2ff; background: rgba(9,15,23,.98); box-shadow: 18px 0 52px rgba(0,0,0,.58); }
  .right { left: auto; right: 0; border-left: 1px solid #35c2ff; border-right: 0; box-shadow: -18px 0 52px rgba(0,0,0,.58); }
  .side.is-drawer-open { display: block; }
  #stage { grid-column: 1; }
  #jump { display: none; }
}
@media (max-width: 640px) {
  #workbench { grid-template-columns: 1fr; }
  .side { width: min(310px, calc(100vw - 42px)); }
  .side:not(.is-drawer-open) { display: none; }
  #minimap { display: none; }
  #graph-overview { display: none; }
  #topbar > button, .debug-controls button { min-height: 28px; padding-inline: 6px; }
}
</style>
</head>
<body>
<div id="shell">
  <header id="topbar">
    <div id="brand"><strong>Jet Canvas</strong><span>source-backed blueprint</span></div>
    <button id="graph-back" title="Back graph">‹</button>
    <button id="graph-forward" title="Forward graph">›</button>
    <select id="graph-select" aria-label="Graph"></select>
    <button id="fit">Fit</button>
    <button id="reload">Reload</button>
    <button id="source-diff">Diff</button>
    <button id="view-toggle">Code</button>
    <button id="developer-mode" title="Show Canvas internals">Developer</button>
    <button id="undo-edit">Undo</button>
    <button id="redo-edit">Redo</button>
    <span id="jump">loading graph</span>
    <div class="debug-controls">
      <select id="debug-session" aria-label="Debug session"><option>local debug</option></select>
      <button id="debug-break">Break</button>
      <button id="debug-watch">Watch</button>
      <button id="debug-step">Step</button>
      <button id="debug-next">Next</button>
      <button id="debug-continue">Continue</button>
      <button id="debug-stop">Stop</button>
    </div>
  </header>
  <main id="workbench">
    <aside id="left-drawer" class="side">
      <section id="graphs-panel" class="panel"><details open><summary><span>Functions</span><span id="graph-count" class="count">0</span></summary><div id="graph-list" class="graph-list"></div></details></section>
      <section id="search-panel" class="panel"><details><summary><span>Search</span></summary><input id="canvas-search" class="search" placeholder="Find in graph"><div id="search-results" class="search-results"></div></details></section>
    </aside>
    <section id="stage">
      <div id="canvas-dock" aria-label="Canvas panels"><button id="dock-graphs">Graphs</button><button id="dock-details">Inspector</button></div>
      <div id="graph-strip" aria-label="Graph tabs"></div>
      <div id="wire-status" aria-live="polite"><span id="wire-status-dot"></span><b>Ready</b><span>Drag from a socket or right-click the canvas</span></div>
      <div id="graph-overview" aria-label="Graph overview"></div>
      <canvas id="jet-canvas-view" width="1400" height="900"></canvas>
      <pre id="source-view" aria-label="Jet source"></pre>
      <canvas id="minimap" width="190" height="124"></canvas>
      <div id="hud"><span id="zoom-label">100%</span><span id="graph-meta">0 nodes</span></div>
    </section>
    <aside id="right-drawer" class="side right">
      <section id="details" class="panel"></section>
    </aside>
  </main>
  <footer id="statusbar"><span id="source-id">source</span><span id="revision">revision</span><span id="schema">canvas v1</span><span id="scm-state">git</span><span id="toast"></span></footer>
</div>
<div id="context-menu" role="menu"></div>
__JET_CANVAS_BOOTSTRAP__
</body>
</html>
"#
    .replace("__JET_CANVAS_BOOTSTRAP__", bootstrap)
}

pub fn canvas_js() -> String {
    r###"(function () {
  const canvas = document.getElementById("jet-canvas-view");
  const ctx = canvas.getContext("2d");
  const stage = document.getElementById("stage");
  const sourceView = document.getElementById("source-view");
  const viewToggle = document.getElementById("view-toggle");
  const developerModeButton = document.getElementById("developer-mode");
  const contextMenu = document.getElementById("context-menu");
  const minimap = document.getElementById("minimap");
  const mini = minimap.getContext("2d");
  const details = document.getElementById("details");
  const jump = document.getElementById("jump");
  const graphBack = document.getElementById("graph-back");
  const graphForward = document.getElementById("graph-forward");
  const graphSelect = document.getElementById("graph-select");
  const graphStrip = document.getElementById("graph-strip");
  const graphCount = document.getElementById("graph-count");
  const wireStatus = document.getElementById("wire-status");
  const graphOverview = document.getElementById("graph-overview");
  const leftDrawer = document.getElementById("left-drawer");
  const rightDrawer = document.getElementById("right-drawer");
  const dockGraphs = document.getElementById("dock-graphs");
  const dockDetails = document.getElementById("dock-details");
  const graphList = document.getElementById("graph-list");
  const canvasSearch = document.getElementById("canvas-search");
  const searchResults = document.getElementById("search-results");
  const zoomLabel = document.getElementById("zoom-label");
  const graphMeta = document.getElementById("graph-meta");
  const sourceId = document.getElementById("source-id");
  const revision = document.getElementById("revision");
  const scmState = document.getElementById("scm-state");
  const toast = document.getElementById("toast");
  const sourceDiff = document.getElementById("source-diff");
  const undoEdit = document.getElementById("undo-edit");
  const redoEdit = document.getElementById("redo-edit");
  const debugStep = document.getElementById("debug-step");
  const debugNext = document.getElementById("debug-next");
  const debugContinue = document.getElementById("debug-continue");
  const debugStop = document.getElementById("debug-stop");
  const debugBreak = document.getElementById("debug-break");
  const debugWatch = document.getElementById("debug-watch");
  let hit = [];
  let pinPoints = new Map();
  let pinHit = [];
  let latestDoc = null;
  let debugOverlay = null;
  let debugState = { breakpoints: [], watches: [] };
  let searchState = { results: [], spans: [], active: -1, diff: null, impact: null };
  let scm = null;
  let undoStack = [];
  let redoStack = [];
  let selectedGraphId = null;
  let graphBackStack = [];
  let graphForwardStack = [];
  let selectedNodeId = null;
  let selectedNodeIds = new Set();
  let view = { x: 64, y: 42, zoom: 1 };
  let drag = null;
  let nodeOffsets = new Map();
  let autoNodeOffsets = new Map();
  let hoverPin = null;
  let pendingPin = null;
  let spaceDown = false;
  let viewMode = "graph";
  let developerMode = false;
  const layoutScale = { x: 1.08, y: 1.08 };
  const palette = [
    { title: "Print", detail: "Call print(\"canvas\")", op: "insert_print" },
    { title: "Branch", detail: "Insert if/else rails", op: "insert_branch" },
    { title: "Switch", detail: "Insert dispatch rails", op: "insert_switch" },
    { title: "Loop", detail: "Insert loop rail", op: "insert_loop" },
    { title: "Fallible", detail: "Insert ? rail", op: "insert_fallible_rail" },
    { title: "Call", detail: "Insert call transaction", op: "insert_call" },
    { title: "Comment", detail: "Source comment projection", op: "comment" }
  ];
  let actionEntries = [];
  let contextMenuState = null;

  function storedFlag(name) {
    try { return window.localStorage && window.localStorage.getItem(name) === "1"; }
    catch (_) { return false; }
  }

  function storeFlag(name, value) {
    try { if (window.localStorage) window.localStorage.setItem(name, value ? "1" : "0"); }
    catch (_) {}
  }

  function setDeveloperMode(on) {
    developerMode = !!on;
    document.body.classList.toggle("is-dev-mode", developerMode);
    developerModeButton.classList.toggle("is-active", developerMode);
    developerModeButton.textContent = developerMode ? "Developer on" : "Developer";
    developerModeButton.title = developerMode ? "Hide Canvas internals" : "Show Canvas internals";
    storeFlag("jet.canvas.developerMode", developerMode);
    if (latestDoc) window.requestAnimationFrame(fitGraph);
  }

  function showToast(text) {
    toast.textContent = text;
    window.clearTimeout(showToast.timer);
    showToast.timer = window.setTimeout(() => { toast.textContent = ""; }, 2200);
  }

  function setPendingPin(pin) {
    pendingPin = pin;
    window.__jetCanvasPendingPin = pin ? { pin_id: pin.pin_id, name: pin.name, type: pin.type, direction: pin.direction } : null;
    syncWireStatus(pin ? { title: "Source socket", detail: pinName(pin) + " : " + exactPinType(pin), color: colorForType(pin.type || "Value") } : null);
  }

  function compactCanvasMode() {
    return window.matchMedia && window.matchMedia("(max-width: 900px)").matches;
  }

  function setDrawer(which) {
    const compact = compactCanvasMode();
    if (!compact) {
      leftDrawer.classList.remove("is-drawer-open");
      rightDrawer.classList.remove("is-drawer-open");
      dockGraphs.classList.remove("is-active");
      dockDetails.classList.remove("is-active");
      return;
    }
    const leftOpen = which === "graphs";
    leftDrawer.classList.toggle("is-drawer-open", leftOpen);
    rightDrawer.classList.toggle("is-drawer-open", which === "details");
    dockGraphs.classList.toggle("is-active", which === "graphs");
    dockDetails.classList.toggle("is-active", which === "details");
    if (latestDoc) window.requestAnimationFrame(fitGraph);
  }

  function fit() {
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(640, Math.floor(rect.width * dpr));
    canvas.height = Math.max(420, Math.floor(rect.height * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  function cssSize() {
    const rect = canvas.getBoundingClientRect();
    return { width: rect.width || 640, height: rect.height || 420 };
  }

  function sx(x) { return x * view.zoom + view.x; }
  function sy(y) { return y * view.zoom + view.y; }
  function wx(x) { return (x - view.x) / view.zoom; }
  function wy(y) { return (y - view.y) / view.zoom; }
  function nodeOffset(node) { return nodeOffsets.get(node.node_id) || { x: 0, y: 0 }; }
  function autoNodeOffset(node) { return autoNodeOffsets.get(node.node_id) || { x: 0, y: 0 }; }
  function rawNodeX(node) { const o = nodeOffset(node); return node.layout.x * layoutScale.x + o.x; }
  function rawNodeY(node) { const o = nodeOffset(node); return node.layout.y * layoutScale.y + o.y; }
  function nodeX(node) { const o = autoNodeOffset(node); return rawNodeX(node) + o.x; }
  function nodeY(node) { const o = autoNodeOffset(node); return rawNodeY(node) + o.y; }

  function colorForType(type) {
    if (type === "Bool") return "#ef4444";
    if (type === "String") return "#22c55e";
    if (type === "Int" || type === "I64" || type === "U64") return "#38bdf8";
    if (type === "Float" || type === "F32" || type === "F64") return "#2dd4bf";
    if (type === "Void" || type === "control" || type === "exec") return "#f8fafc";
    if (String(type || "").endsWith("?")) return "#fb7185";
    if (String(type || "").startsWith("[")) return "#f59e0b";
    return "#a78bfa";
  }

  function wireColor(wire, from) {
    if (wire.wire_kind === "control") return "#f8fafc";
    if (wire.wire_kind === "fallible") return "#fb7185";
    if (wire.wire_kind === "effect") return "#c084fc";
    if (wire.wire_kind === "debug") return "#facc15";
    return from.color;
  }

  function pinRail(pin) {
    if (!pin) return "data";
    if ((pin.type || "") === "Void" || pin.name === "exec" || pin.capability === "control") return "control";
    if (pin.fallible) return "fallible";
    if (pin.effect_grant_need) return "effect";
    return "data";
  }

  function setViewMode(mode) {
    viewMode = mode === "source" ? "source" : "graph";
    stage.classList.toggle("is-source", viewMode === "source");
    viewToggle.textContent = viewMode === "source" ? "Nodes" : "Code";
    viewToggle.classList.toggle("is-active", viewMode === "source");
    sourceView.textContent = latestDoc && latestDoc.source_text ? latestDoc.source_text : "";
    if (viewMode === "graph" && latestDoc) drawGraph(latestDoc);
  }

  function hexToRgba(hex, alpha) {
    const h = String(hex || "#2563eb").replace("#", "");
    const r = parseInt(h.slice(0, 2), 16) || 37;
    const g = parseInt(h.slice(2, 4), 16) || 99;
    const b = parseInt(h.slice(4, 6), 16) || 235;
    return "rgba(" + r + "," + g + "," + b + "," + (parseFloat(alpha) || 0.18) + ")";
  }

  function spansOverlap(a, b) {
    if (!a || !b) return false;
    return a.start <= b.end && b.start <= a.end;
  }

  function postQuery(body) {
    if (!latestDoc) return Promise.resolve(null);
    return fetch(queryUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(Object.assign({ schema_version: 1, revision: latestDoc.revision }, body))
    })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) {
          showToast(result.json.message || "Canvas query rejected");
          return null;
        }
        searchState.results = result.json.results || [];
        searchState.spans = searchState.results.map((r) => r.source_span).filter(Boolean);
        searchState.active = searchState.results.length ? 0 : -1;
        searchState.diff = result.json.diff || null;
        searchState.impact = result.json.impact || null;
        renderSearchResults();
        if (searchState.results[0]) selectQueryResult(searchState.results[0], false);
        if (latestDoc) drawGraph(latestDoc);
        return result.json;
      })
      .catch((e) => { showToast(String(e)); return null; });
  }

  function renderSearchResults() {
    const rows = (searchState.results || []).slice(0, 24).map((result, i) => {
      const active = i === searchState.active ? " is-active" : "";
      const label = escapeHtml(result.title || result.symbol || result.kind || "match");
      const where = `${result.kind || "match"} · line ${result.line || "?"}`;
      return `<button class="search-item${active}" data-search-hit="${i}">${label}<small>${escapeHtml(where)} ${escapeHtml(result.excerpt || "")}</small></button>`;
    }).join("");
    const diff = searchState.diff && searchState.diff.text ? `<div class="inline-row"><b>Preview diff</b><code>${escapeHtml(searchState.diff.text)}</code></div>` : "";
    const impact = searchState.impact && searchState.impact.found ? `<div class="pin-row"><b>Impact</b><br><span class="tag">${(searchState.impact.references || []).length} refs / ${(searchState.impact.call_sites || []).length} calls</span></div>` : "";
    searchResults.innerHTML = rows || diff || impact ? rows + diff + impact : "<div class=\"tag\">no matches</div>";
    searchResults.querySelectorAll("[data-search-hit]").forEach((button) => {
      button.addEventListener("click", () => {
        const index = Number(button.getAttribute("data-search-hit"));
        searchState.active = index;
        selectQueryResult(searchState.results[index], true);
        renderSearchResults();
        if (latestDoc) drawGraph(latestDoc);
      });
    });
  }

  function loadSourceControl() {
    return fetch(sourceControlUrl, { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        scm = doc;
        scmState.textContent = doc.available ? (doc.dirty ? "git dirty" : "git clean") : "no git";
        scmState.style.color = doc.dirty ? "#fde68a" : "#8fb2dc";
        return doc;
      })
      .catch(() => {
        scm = null;
        scmState.textContent = "git ?";
      });
  }

  function showSourceDiff() {
    const render = (doc) => {
      if (!doc) return;
      const diff = doc.diff || (doc.dirty ? doc.status : "clean");
      searchState.results = [];
      searchState.spans = [];
      searchState.active = -1;
      searchState.impact = null;
      searchState.diff = { text: diff || "clean" };
      renderSearchResults();
      showToast(doc.dirty ? "Source diff loaded" : "Source clean");
    };
    if (scm) render(scm);
    else loadSourceControl().then(render);
  }

  function selectQueryResult(result, fitView) {
    if (!result) return;
    if (result.graph_id) selectedGraphId = result.graph_id;
    if (result.node_id) {
      selectedNodeId = result.node_id;
      selectedNodeIds = new Set([result.node_id]);
    }
    if (fitView && latestDoc) fitGraph();
    if (result.source_span) setSourceHash(result.source_span);
  }

  function runCanvasSearch() {
    const query = canvasSearch.value.trim();
    if (!query) {
      searchState = { results: [], spans: [], active: -1, diff: null, impact: null };
      renderSearchResults();
      if (latestDoc) drawGraph(latestDoc);
      return;
    }
    postQuery({ op: "find", query });
  }

  function sourceHashSpan() {
    const m = String(window.location.hash || "").match(/^#span-(\d+)-(\d+)$/);
    return m ? { start: Number(m[1]), end: Number(m[2]) } : null;
  }

  function setSourceHash(span) {
    if (!span) return;
    const next = "#span-" + span.start + "-" + span.end;
    if (window.location.hash === next) return;
    window.history.replaceState(null, "", next);
  }

  function applySourceHash() {
    const span = sourceHashSpan();
    if (!span || !latestDoc) return;
    postQuery({ op: "source_to_graph", start: span.start, end: span.end });
  }

  function roundRect(x, y, w, h, r) {
    const rr = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + rr, y);
    ctx.lineTo(x + w - rr, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + rr);
    ctx.lineTo(x + w, y + h - rr);
    ctx.quadraticCurveTo(x + w, y + h, x + w - rr, y + h);
    ctx.lineTo(x + rr, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - rr);
    ctx.lineTo(x, y + rr);
    ctx.quadraticCurveTo(x, y, x + rr, y);
  }

  function drawPin(pin, x, y, dir, recordHit = true) {
    const color = colorForType(pin.type || "unknown");
    const rail = pinRail(pin);
    const r = isExecPin(pin) ? Math.max(7, 9 * view.zoom) : Math.max(5.5, 7 * view.zoom);
    ctx.beginPath();
    if (rail === "control") {
      ctx.moveTo(x - r * .9, y - r);
      ctx.lineTo(x + r * 1.25, y);
      ctx.lineTo(x - r * .9, y + r);
      ctx.closePath();
    } else if (rail === "fallible") {
      ctx.moveTo(x, y - r); ctx.lineTo(x + r, y); ctx.lineTo(x, y + r); ctx.lineTo(x - r, y); ctx.closePath();
    } else {
      ctx.arc(x, y, r * .86, 0, Math.PI * 2);
    }
    ctx.fillStyle = color;
    ctx.fill();
    if (hoverPin && hoverPin.pin_id === pin.pin_id) {
      ctx.beginPath();
      ctx.arc(x, y, 10, 0, Math.PI * 2);
      ctx.strokeStyle = "#fef08a";
      ctx.lineWidth = 2;
      ctx.stroke();
    }
    ctx.lineWidth = Math.max(1.4, 1.8 * view.zoom);
    ctx.strokeStyle = isExecPin(pin) ? "#02060a" : "rgba(2,6,10,.9)";
    ctx.stroke();
    if (recordHit) {
      const hitR = Math.max(18, 23 * view.zoom);
      pinPoints.set(pin.pin_id, { x, y, color, pin });
      pinHit.push({ x: x - hitR, y: y - hitR, w: hitR * 2, h: hitR * 2, cx: x, cy: y, pin });
    }
  }

  function pinName(pin) {
    if (!pin) return "";
    return pin.name || "";
  }

  function exactPinType(pin) {
    if (!pin) return "";
    return isExecPin(pin) ? "exec" : (pin.type || "Value");
  }

  function clipText(text, max) {
    const s = String(text || "");
    return s.length > max ? s.slice(0, Math.max(1, max - 1)) + "…" : s;
  }

  function drawPinLabel(pin, x, y, align) {
    const label = pinName(pin);
    ctx.font = `${Math.max(9, 11.5 * view.zoom)}px ui-monospace, Consolas, monospace`;
    ctx.textAlign = align;
    ctx.fillStyle = isExecPin(pin) ? "#edf6ff" : colorForType(pin.type || "unknown");
    ctx.fillText(clipText(label, 24), x, y);
    ctx.textAlign = "left";
  }

  function drawPinHoverTooltip(pin) {
    const point = pin && pinPoints.get(pin.pin_id);
    if (!point) return;
    const text = pinName(pin) + " : " + exactPinType(pin);
    ctx.font = `${Math.max(10, 12 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 9 * view.zoom;
    const badgeW = Math.min(220 * view.zoom, ctx.measureText(text).width + padX * 2);
    const badgeH = 25 * view.zoom;
    const bx = Math.max(8, Math.min(point.x + 14 * view.zoom, cssSize().width - badgeW - 8));
    const by = Math.max(8, point.y - badgeH - 13 * view.zoom);
    roundRect(bx, by, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.95)";
    ctx.fill();
    ctx.strokeStyle = point.color;
    ctx.lineWidth = Math.max(1, view.zoom);
    ctx.stroke();
    ctx.fillStyle = point.color;
    ctx.textAlign = "left";
    ctx.fillText(clipText(text, 28), bx + padX, by + 17 * view.zoom);
  }

  function drawSocketRow(pin, x, y, w, dir, recordHit) {
    const rowH = 24 * view.zoom;
    const px = dir === "input" ? x : x + w;
    const inset = 12 * view.zoom;
    const labelX = dir === "input" ? x + 28 * view.zoom : x + w - 28 * view.zoom;
    const labelAlign = dir === "input" ? "left" : "right";
    if (isExecPin(pin)) {
      const railStart = dir === "input" ? x + 10 * view.zoom : x + w - 126 * view.zoom;
      const railEnd = dir === "input" ? x + 126 * view.zoom : x + w - 10 * view.zoom;
      ctx.beginPath();
      ctx.moveTo(railStart, y);
      ctx.lineTo(railEnd, y);
      ctx.strokeStyle = "rgba(248,250,252,.52)";
      ctx.lineWidth = Math.max(1.8, 2.4 * view.zoom);
      ctx.stroke();
      drawPin(pin, px, y, dir, recordHit);
      drawPinLabel(pin, labelX, y + 4 * view.zoom, labelAlign);
      return;
    }
    ctx.beginPath();
    roundRect(x + inset, y - rowH * .56, w - inset * 2, rowH, 4 * view.zoom);
    ctx.fillStyle = hexToRgba(colorForType(pin.type), .10);
    ctx.fill();
    ctx.strokeStyle = hexToRgba(colorForType(pin.type), .28);
    ctx.lineWidth = Math.max(.8, view.zoom);
    ctx.stroke();
    drawPin(pin, px, y, dir, recordHit);
    drawPinLabel(pin, labelX, y + 4 * view.zoom, labelAlign);
  }

  function compatibleActionType(accepted, actual) {
    if (!accepted || !actual) return true;
    if (accepted === actual) return true;
    if (accepted === "Any" || accepted === "Value") return true;
    if (actual === "Any" || actual === "Value") return true;
    return numericType(accepted) && numericType(actual);
  }

  function functionsForPin(pin) {
    if (!pin) return actionEntries.slice(0, 8);
    const targetType = pin.type || null;
    let entries = actionEntries.filter((entry) => {
      if (!targetType) return true;
      return (entry.pins || []).some((p) => p.direction === "input" && compatibleActionType(p.type, targetType));
    });
    if (entries.length === 0) entries = actionEntries;
    return entries.slice(0, 8);
  }

  function closeContextMenu() {
    contextMenu.classList.remove("is-open");
    contextMenu.innerHTML = "";
    contextMenuState = null;
  }

  function actionMatchesQuery(action, query) {
    if (!query) return true;
    const q = query.toLowerCase();
    return String(action.title || "").toLowerCase().includes(q) || String(action.detail || "").toLowerCase().includes(q) || String(action.group || "").toLowerCase().includes(q);
  }

  function renderActionPalette() {
    if (!contextMenuState) return;
    const query = contextMenuState.query || "";
    const matches = contextMenuState.actions.filter((action) => actionMatchesQuery(action, query)).slice(0, 18);
    const context = contextMenuState.pin ? `${contextMenuState.pin.name}: ${contextMenuState.pin.type}` : (contextMenuState.context || "source-backed actions");
    const port = contextMenuState.pin ? pinPortHtml(contextMenuState.pin.type || "Value") : "";
    contextMenu.innerHTML = `<div class="action-palette-head"><div class="menu-title">${escapeHtml(contextMenuState.title)}</div><div class="action-context">${port}<span>${escapeHtml(context)}</span><span class="tag">${matches.length}/${contextMenuState.actions.length}</span></div><input id="action-palette-search" placeholder="Search actions" value="${escapeAttr(query)}"></div><div class="action-results">${matches.length ? matches.map((action) => `<button class="action-result" data-menu-action="${escapeAttr(action.index)}"><span>${escapeHtml(action.title)}<small>${escapeHtml(action.detail || "")}</small></span><span class="tag">${escapeHtml(action.group || "")}</span></button>`).join("") : "<div class=\"action-empty\">No matching actions</div>"}</div>`;
    const input = document.getElementById("action-palette-search");
    if (input) {
      input.addEventListener("input", () => {
        contextMenuState.query = input.value || "";
        renderActionPalette();
        const next = document.getElementById("action-palette-search");
        if (next) {
          next.focus();
          next.setSelectionRange(next.value.length, next.value.length);
        }
      });
      input.addEventListener("keydown", (ev) => {
        if (ev.key === "Escape") {
          ev.preventDefault();
          closeContextMenu();
        } else if (ev.key === "Enter") {
          const first = contextMenu.querySelector("[data-menu-action]");
          if (first) {
            ev.preventDefault();
            first.click();
          }
        }
      });
    }
    contextMenu.querySelectorAll("[data-menu-action]").forEach((button) => {
      button.addEventListener("click", () => {
        const action = contextMenuState && contextMenuState.actions.find((item) => String(item.index) === button.getAttribute("data-menu-action"));
        closeContextMenu();
        if (action) action.run();
      });
    });
  }

  function openActionPalette(x, y, title, actions, opts = {}) {
    contextMenuState = {
      title,
      actions: (actions.length ? actions : [{ title: "No compatible actions", detail: "source-backed only", group: "empty", run: () => {} }]).map((action, index) => Object.assign({ index }, action)),
      pin: opts.pin || null,
      context: opts.context || "",
      query: opts.query || ""
    };
    renderActionPalette();
    contextMenu.style.left = Math.min(x, window.innerWidth - 430) + "px";
    contextMenu.style.top = Math.min(y, window.innerHeight - 430) + "px";
    contextMenu.classList.add("is-open");
    const input = document.getElementById("action-palette-search");
    if (input) input.focus();
  }

  function openContextMenu(x, y, title, actions) {
    openActionPalette(x, y, title, actions, { context: "node actions" });
  }

  function openPinMenu(pin, x, y) {
    const entries = functionsForPin(pin);
    const actions = entries.map((entry) => ({
      title: entry.title,
      detail: entry.detail,
      group: "call",
      run: () => runPalette(entry)
    }));
    actions.unshift({
      title: "Create function accepting " + (pin.type || "Value"),
      detail: pin.name || "value",
      group: "function",
      run: () => {
        const base = String(pin.name || "value").replace(/[^A-Za-z0-9_]/g, "_").replace(/^[^A-Za-z_]+/, "") || "value";
        const name = window.prompt("Function name", "use_" + base);
        if (name) postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params: "value: " + (pin.type || "Int"), ret_type: "Void" });
      }
    });
    if (pin.direction === "input") {
      actions.unshift({
        title: "Promote pin to binding",
        detail: pin.type || "value",
        group: "refactor",
        run: () => {
          const name = window.prompt("Binding name", pin.name || "value");
          const graph = latestDoc ? currentGraph(latestDoc) : null;
          const expr = graph && (graph.inline_exprs || []).find((e) => e.source_span && pin.source_span && spansOverlap(e.source_span, pin.source_span));
          if (name && expr) postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, name });
          else showToast("Select an inline expression to promote");
        }
      });
    }
    openActionPalette(x, y, "Compatible actions", actions, { pin, context: "pin actions" });
  }

  function richestGraph(doc) {
    if (!doc.graphs || doc.graphs.length === 0) return null;
    return doc.graphs.slice().sort((a, b) => b.nodes.length - a.nodes.length || a.title.localeCompare(b.title))[0];
  }

  function syncGraphPicker(doc) {
    const best = richestGraph(doc);
    if (!selectedGraphId && best) selectedGraphId = best.graph_id;
    graphSelect.innerHTML = "";
    for (const graph of doc.graphs || []) {
      const opt = document.createElement("option");
      opt.value = graph.graph_id;
      opt.textContent = graph.title + " (" + graph.nodes.length + ")";
      graphSelect.appendChild(opt);
    }
    if (selectedGraphId) graphSelect.value = selectedGraphId;
  }

  function syncGraphList(doc) {
    graphList.innerHTML = "";
    for (const graph of doc.graphs || []) {
      const button = document.createElement("button");
      button.className = "graph-item" + (graph.graph_id === selectedGraphId ? " is-active" : "");
      button.type = "button";
      button.setAttribute("data-sidebar-graph", graph.graph_id);
      button.innerHTML = "<span>" + escapeHtml(graph.title) + "</span><span class=\"count\">" + graph.nodes.length + "</span>";
      button.addEventListener("click", () => {
        switchGraph(graph.graph_id);
      });
      graphList.appendChild(button);
    }
  }

  function syncWireStatus(state) {
    if (!wireStatus) return;
    let title = "Ready";
    let detail = "Drag from a socket or right-click the canvas";
    let color = "#7dd3fc";
    if (state) {
      title = state.title || title;
      detail = state.detail || detail;
      color = state.color || color;
    } else if (hoverPin) {
      title = "Socket";
      detail = pinName(hoverPin) + " : " + exactPinType(hoverPin);
      color = colorForType(hoverPin.type || "Value");
    }
    wireStatus.style.setProperty("--wire-color", color);
    wireStatus.innerHTML = `<span id="wire-status-dot"></span><b>${escapeHtml(title)}</b><span>${escapeHtml(detail)}</span>`;
  }

  function graphRailSummary(graph) {
    return graph && graph.rails && graph.rails.kinds && graph.rails.kinds.length ? graph.rails.kinds.join(", ") : "data";
  }

  function syncGraphOverview(graph, node) {
    if (!graphOverview || !graph) return;
    const fn = graph.function || {};
    const signature = fn.signature || graph.title || graph.graph_id;
    const execPins = (graph.pins || []).filter(isExecPin).length;
    const dataPins = (graph.pins || []).length - execPins;
    const selected = node ? nodeKindLabel(node, graph) + " / " + node.title : "none";
    graphOverview.innerHTML = `
      <div class="graph-overview-title"><b>${escapeHtml(graph.title || graph.graph_id)}</b><code>${escapeHtml(signature)}</code></div>
      <div class="graph-stats">
        <div class="graph-stat"><b>${graph.nodes.length}</b><span>nodes</span></div>
        <div class="graph-stat"><b>${graph.wires.length}</b><span>wires</span></div>
        <div class="graph-stat"><b>${dataPins}/${execPins}</b><span>data/exec</span></div>
      </div>
      <div class="tag">${escapeHtml(graphRailSummary(graph))} / ${escapeHtml(selected)}</div>
    `;
    window.__jetCanvasGraphOverview = {
      graph_id: graph.graph_id,
      title: graph.title || graph.graph_id,
      nodes: graph.nodes.length,
      wires: graph.wires.length,
      data_pins: dataPins,
      exec_pins: execPins,
      selected
    };
  }

  function nodeContextActions(graph, node) {
    const actions = [
      { title: "Jump source", detail: "span", group: "source", run: () => { const s = node.source_span || { start: 0, end: 0 }; setSourceHash(s); setViewMode("source"); } },
      { title: "Find references", detail: "semindex", group: "query", run: () => postQuery({ op: "references", symbol: node.title }) },
      { title: "Set breakpoint", detail: "local span", group: "debug", run: () => toggleBreakpoint(node) }
    ];
    if (graphForFunctionName(node.title)) actions.unshift({ title: "Open function graph", detail: "nested graph", group: "graph", run: () => openFunctionGraph(node.title) });
    return actions;
  }

  function syncGraphStrip(doc) {
    graphStrip.innerHTML = "";
    if (graphCount) graphCount.textContent = String((doc.graphs || []).length);
    for (const graph of doc.graphs || []) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "graph-tab" + (graph.graph_id === selectedGraphId ? " is-active" : "");
      button.setAttribute("data-graph-tab", graph.graph_id);
      button.title = "Open graph: " + graph.title;
      button.innerHTML = "<span class=\"graph-tab-kind\">fn</span><span class=\"graph-tab-title\">" + escapeHtml(graph.title) + "</span><span class=\"graph-tab-count\">" + graph.nodes.length + "</span>";
      button.addEventListener("click", (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        switchGraph(graph.graph_id);
      });
      graphStrip.appendChild(button);
    }
    window.__jetCanvasGraphSwitcherReady = true;
    window.__jetCanvasGraphTabCount = graphStrip.children.length;
  }

  function graphActionItems() {
    const actions = [
      { title: "Fit graph", detail: "viewport", group: "view", run: fitGraph },
      { title: "New function", detail: "source", group: "function", run: () => {
        const name = window.prompt("Function name", "helper");
        if (name) postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params: "", ret_type: "Int" });
      } },
      { title: "Show source", detail: "toggle", group: "view", run: () => setViewMode("source") }
    ];
    for (const item of palette.concat(actionEntries).slice(0, 36)) {
      actions.push({ title: item.title, detail: item.detail || "", group: item.op === "preview_canvas_action" ? "built-in" : "node", run: () => runPalette(item) });
    }
    return actions;
  }

  function openGraphActionPalette(x, y, query) {
    openActionPalette(x, y, "Graph actions", graphActionItems(), { context: "right-click built-ins, functions, source actions", query: query || "" });
  }

  function currentGraph(doc) {
    return (doc.graphs || []).find((g) => g.graph_id === selectedGraphId) || richestGraph(doc);
  }

  function graphById(id) {
    return latestDoc && (latestDoc.graphs || []).find((graph) => graph.graph_id === id) || null;
  }

  function graphTitle(id) {
    const graph = graphById(id);
    return graph ? graph.title : "graph";
  }

  function updateGraphNav(graph) {
    graphBack.disabled = graphBackStack.length === 0;
    graphForward.disabled = graphForwardStack.length === 0;
    const trail = graphBackStack.slice(-2).map(graphTitle).concat(graph ? [graph.title] : []);
    jump.textContent = trail.join("  ›  ") + (graph ? " - " + graph.nodes.length + " nodes" + (selectedNodeIds.size > 1 ? " / " + selectedNodeIds.size + " selected" : "") : "");
  }

  function switchGraph(graphId, opts = {}) {
    if (!latestDoc || !graphId) return;
    const graph = graphById(graphId);
    if (!graph) return;
    const push = opts.push !== false && selectedGraphId && selectedGraphId !== graphId;
    if (push) {
      graphBackStack.push(selectedGraphId);
      graphForwardStack = [];
    }
    selectedGraphId = graphId;
    window.__jetCanvasSelectedGraphId = selectedGraphId;
    selectedNodeId = opts.nodeId || graph.entry_node;
    selectedNodeIds = new Set([selectedNodeId]);
    setViewMode("graph");
    if (opts.fit === false) drawGraph(latestDoc);
    else fitGraph();
    if (opts.toast) showToast(opts.toast);
  }

  function graphForFunctionName(name) {
    if (!latestDoc || !name) return null;
    return (latestDoc.graphs || []).find((graph) => graph.title === name) || null;
  }

  function openFunctionGraph(name) {
    const graph = graphForFunctionName(name);
    if (!graph) return false;
    switchGraph(graph.graph_id, { toast: "Opened " + graph.title });
    return true;
  }

  function debugStorageKey(doc) {
    return "jet.canvas.debug:" + (doc.source_id || "source");
  }

  function loadDebugState(doc) {
    const key = debugStorageKey(doc);
    if (debugState.key === key && debugState.revision === doc.revision) return;
    try {
      debugState = JSON.parse(localStorage.getItem(key) || "null") || { breakpoints: [], watches: [] };
    } catch (_) {
      debugState = { breakpoints: [], watches: [] };
    }
    debugState.key = key;
    debugState.revision = doc.revision;
    debugState.breakpoints = (debugState.breakpoints || []).filter((b) => b.revision === doc.revision);
    debugState.watches = debugState.watches || [];
  }

  function saveDebugState() {
    if (!debugState.key) return;
    localStorage.setItem(debugState.key, JSON.stringify({
      breakpoints: debugState.breakpoints || [],
      watches: debugState.watches || [],
      revision: debugState.revision
    }));
  }

  function spanAnchor(span) {
    if (!span) return "";
    return String(span.start) + ":" + String(span.end);
  }

  function nodeBreakpoint(node) {
    const anchor = spanAnchor(node && node.source_span);
    return (debugState.breakpoints || []).find((b) => b.anchor === anchor);
  }

  function toggleBreakpoint(node) {
    if (!latestDoc || !node || !node.source_span) return;
    loadDebugState(latestDoc);
    const anchor = spanAnchor(node.source_span);
    const before = debugState.breakpoints.length;
    debugState.breakpoints = debugState.breakpoints.filter((b) => b.anchor !== anchor);
    if (debugState.breakpoints.length === before) {
      debugState.breakpoints.push({ anchor, source_span: node.source_span, node_id: node.node_id, revision: latestDoc.revision });
      showToast("Breakpoint anchored to source span");
    } else {
      showToast("Breakpoint removed");
    }
    saveDebugState();
    drawGraph(latestDoc);
  }

  function addWatch(name) {
    if (!name) return;
    loadDebugState(latestDoc);
    if (!debugState.watches.includes(name)) debugState.watches.push(name);
    saveDebugState();
    showToast("Watch added: " + name);
  }

  function runDebug(commands) {
    if (!latestDoc) return;
    loadDebugState(latestDoc);
    const debugUrl = window.__JET_CANVAS_DEBUG__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/debug");
    const body = {
      schema_version: 1,
      revision: latestDoc.revision,
      commands,
      breakpoint_spans: (debugState.breakpoints || []).map((b) => b.anchor),
      watches: debugState.watches || []
    };
    fetch(debugUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) {
          debugOverlay = null;
          showToast((result.json.message || "Debug rejected").split("\n")[0]);
          return;
        }
        debugOverlay = result.json.overlay || null;
        if (debugOverlay && debugOverlay.active_graph_id) selectedGraphId = debugOverlay.active_graph_id;
        if (debugOverlay && debugOverlay.active_node_id) selectedNodeId = debugOverlay.active_node_id;
        showToast("Debug " + ((debugOverlay && debugOverlay.debug_overlay) || "updated"));
        drawGraph(latestDoc);
      })
      .catch((e) => showToast(String(e)));
  }

  function stopDebug() {
    debugOverlay = null;
    showToast("Debug overlay stopped");
    if (latestDoc) drawGraph(latestDoc);
  }

  function reflowGraph(graph) {
    autoNodeOffsets = new Map();
    if (!graph || !graph.nodes || graph.nodes.length === 0) return;
    const rowGap = 88;
    const colGap = 22;
    const rows = [];
    for (const node of graph.nodes.slice().sort((a, b) => rawNodeY(a) - rawNodeY(b) || rawNodeX(a) - rawNodeX(b))) {
      let row = rows.find((candidate) => Math.abs(candidate.y - rawNodeY(node)) < rowGap);
      if (!row) {
        row = { y: rawNodeY(node), nodes: [] };
        rows.push(row);
      }
      row.nodes.push(node);
    }
    for (const row of rows) {
      let cursor = -Infinity;
      let previous = null;
      for (const node of row.nodes.sort((a, b) => rawNodeX(a) - rawNodeX(b))) {
        const x = rawNodeX(node);
        const size = nodeSize(graph, node);
        const shift = Math.max(0, cursor - x);
        if (previous && shift > 0 && previous.kind === node.kind) {
          autoNodeOffsets.set(node.node_id, { x: 0, y: size.h + colGap });
          cursor = Math.max(cursor, x + size.w + colGap);
        } else {
          autoNodeOffsets.set(node.node_id, { x: shift, y: 0 });
          cursor = x + shift + size.w + colGap;
          previous = node;
        }
      }
    }
  }

  function graphBounds(graph) {
    if (!graph || graph.nodes.length === 0) return { minX: 0, minY: 0, maxX: 600, maxY: 360 };
    reflowGraph(graph);
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of graph.nodes) {
      const size = nodeSize(graph, n);
      minX = Math.min(minX, nodeX(n));
      minY = Math.min(minY, nodeY(n));
      maxX = Math.max(maxX, nodeX(n) + size.w);
      maxY = Math.max(maxY, nodeY(n) + size.h);
    }
    return { minX, minY, maxX, maxY };
  }

  function fitGraph() {
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    if (!graph) return;
    const b = graphBounds(graph);
    const size = cssSize();
    const compact = compactCanvasMode();
    const leftInset = 22;
    const topInset = compact ? (developerMode ? 154 : 52) : (developerMode ? 108 : 32);
    const bottomInset = 38;
    const zx = (size.width - leftInset - 28) / Math.max(1, b.maxX - b.minX);
    const zy = (size.height - topInset - bottomInset) / Math.max(1, b.maxY - b.minY);
    view.zoom = Math.max(.42, Math.min(1.05, Math.min(zx, zy)));
    view.x = leftInset - b.minX * view.zoom;
    view.y = topInset - b.minY * view.zoom;
    drawGraph(latestDoc);
  }

  function drawGrid(size) {
    const grad = ctx.createRadialGradient(size.width * .5, size.height * .42, 40, size.width * .5, size.height * .42, Math.max(size.width, size.height));
    grad.addColorStop(0, "#101a27");
    grad.addColorStop(.48, "#08111c");
    grad.addColorStop(1, "#05080d");
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, size.width, size.height);
    const major = 96 * view.zoom;
    const minor = 24 * view.zoom;
    ctx.lineWidth = 1;
    for (const step of [minor, major]) {
      if (step < 6) continue;
      ctx.strokeStyle = step === major ? "rgba(56,82,110,.72)" : "rgba(32,43,57,.72)";
      ctx.beginPath();
      let ox = view.x % step;
      let oy = view.y % step;
      for (let x = ox; x < size.width; x += step) { ctx.moveTo(x, 0); ctx.lineTo(x, size.height); }
      for (let y = oy; y < size.height; y += step) { ctx.moveTo(0, y); ctx.lineTo(size.width, y); }
      ctx.stroke();
    }
  }

  function drawTypeLegend(size) {
    if (!developerMode || compactCanvasMode() || size.width < 760 || viewMode !== "graph") return;
    const items = [
      ["Exec", "exec"],
      ["Bool", "Bool"],
      ["Int", "Int"],
      ["Float", "Float"],
      ["String", "String"],
      ["Fallible", "Value?"]
    ];
    const x = Math.max(14, size.width - 430);
    const y = 58;
    const w = Math.min(416, size.width - x - 12);
    const h = 34;
    roundRect(x, y, w, h, 6);
    ctx.fillStyle = "rgba(8,17,29,.78)";
    ctx.fill();
    ctx.strokeStyle = "rgba(54,90,127,.72)";
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.font = "10px ui-monospace, Consolas, monospace";
    ctx.textAlign = "left";
    let cursor = x + 12;
    for (const [label, type] of items) {
      const color = colorForType(type);
      ctx.beginPath();
      if (type === "exec") {
        ctx.moveTo(cursor, y + 12);
        ctx.lineTo(cursor + 10, y + 17);
        ctx.lineTo(cursor, y + 22);
        ctx.closePath();
      } else {
        ctx.arc(cursor + 5, y + 17, 5, 0, Math.PI * 2);
      }
      ctx.fillStyle = color;
      ctx.fill();
      ctx.fillStyle = "#b9c9df";
      ctx.fillText(label, cursor + 15, y + 20);
      cursor += Math.min(72, 22 + ctx.measureText(label).width);
    }
    ctx.textAlign = "left";
  }

  function isExecPin(pin) {
    return pinRail(pin) === "control";
  }

  function pinsForNode(graph, node, direction, exec) {
    return (graph.pins || []).filter((pin) => pin.node_id === node.node_id && pin.direction === direction && isExecPin(pin) === exec);
  }

  function nodeStyle(node, graph) {
    const table = {
      entry: { accent: "#35c2ff", header: "#123247", fill: "#101923", label: "Function", glyph: "FN" },
      call: { accent: "#7c8cff", header: "#20245a", fill: "#111423", label: "Call", glyph: "CALL" },
      method: { accent: "#b48cff", header: "#312052", fill: "#151020", label: "Method", glyph: "M" },
      variant: { accent: "#d58cff", header: "#3a1d4a", fill: "#17101f", label: "Variant", glyph: "V" },
      binding: { accent: "#5ee0a0", header: "#153f30", fill: "#0f1a17", label: "Set variable", glyph: "SET" },
      assign: { accent: "#ffb454", header: "#4a3014", fill: "#1c160e", label: "Assign", glyph: "SET" },
      variable_get: { accent: "#7ee787", header: "#193b24", fill: "#0d1711", label: "Get variable", glyph: "GET" },
      constant: { accent: "#f6d365", header: "#4d3a12", fill: "#1a150a", label: "Constant", glyph: "LIT" },
      branch: { accent: "#ff7b72", header: "#4b1f23", fill: "#1c1114", label: "Branch", glyph: "IF" },
      dispatch: { accent: "#ff9f43", header: "#4b2d0d", fill: "#1b140a", label: "Dispatch", glyph: "==" },
      loop: { accent: "#22d3ee", header: "#123d45", fill: "#0d181c", label: "Loop", glyph: "LOOP" },
      return: { accent: "#6ee7b7", header: "#144335", fill: "#0c1915", label: "Return", glyph: "RET" },
      fallible: { accent: "#fb7185", header: "#4c1722", fill: "#1c1014", label: "Fallible", glyph: "?" },
      flow: { accent: "#f8fafc", header: "#2d3440", fill: "#11161d", label: "Flow", glyph: "GO" },
      yield: { accent: "#67e8f9", header: "#134250", fill: "#0e1a1f", label: "Yield", glyph: "Y" }
    };
    const style = table[node.kind] || { accent: "#a78bfa", header: "#2b2141", fill: "#14111d", label: node.kind || "Node", glyph: "N" };
    if (node.kind === "constant") {
      const out = pinsForNode(graph, node, "output", false)[0];
      return Object.assign({}, style, { accent: colorForType(out && out.type || "unknown") });
    }
    return style;
  }

  function nodeSubtitle(node) {
    if (node.kind === "variable_get") return "read from source";
    if (node.kind === "binding") return "write local binding";
    if (node.kind === "assign") return "mutate existing value";
    if (node.kind === "constant") return "literal";
    return node.kind;
  }

  function nodeKindLabel(node, graph) {
    return nodeStyle(node, graph).label || node.kind || "Node";
  }

  function shouldDrawNodeBadge(node) {
    return false;
  }

  function nodeSize(graph, node) {
    const inputData = pinsForNode(graph, node, "input", false).length;
    const outputData = pinsForNode(graph, node, "output", false).length;
    const execIn = pinsForNode(graph, node, "input", true).length;
    const execOut = pinsForNode(graph, node, "output", true).length;
    const compact = node.kind === "variable_get" || node.kind === "constant";
    const w = compact ? 216 : node.kind === "branch" || node.kind === "dispatch" ? 340 : 314;
    const execRows = Math.max(execIn, execOut);
    const dataRows = Math.max(inputData, outputData);
    const h = compact ? 84 : Math.max(132, 70 + Math.max(1, execRows) * 28 + Math.max(1, dataRows) * 30);
    return { w, h };
  }

  function bezierPoint(from, to, t) {
    const c1 = { x: from.x + 96 * view.zoom, y: from.y };
    const c2 = { x: to.x - 96 * view.zoom, y: to.y };
    const mt = 1 - t;
    return {
      x: mt * mt * mt * from.x + 3 * mt * mt * t * c1.x + 3 * mt * t * t * c2.x + t * t * t * to.x,
      y: mt * mt * mt * from.y + 3 * mt * mt * t * c1.y + 3 * mt * t * t * c2.y + t * t * t * to.y
    };
  }

  function drawWireArrow(from, to, color, control) {
    const p = bezierPoint(from, to, .72);
    const q = bezierPoint(from, to, .66);
    const angle = Math.atan2(p.y - q.y, p.x - q.x);
    const len = (control ? 12 : 9) * view.zoom;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.rotate(angle);
    ctx.beginPath();
    ctx.moveTo(len, 0);
    ctx.lineTo(-len * .55, -len * .55);
    ctx.lineTo(-len * .2, 0);
    ctx.lineTo(-len * .55, len * .55);
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.shadowColor = hexToRgba(color, .55);
    ctx.shadowBlur = control ? 10 : 6;
    ctx.fill();
    ctx.restore();
    ctx.shadowBlur = 0;
  }

  function drawWire(wire, from, to, activeWire, selectedWire) {
    const control = wire.wire_kind === "control";
    const color = activeWire ? "#facc15" : wireColor(wire, from);
    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.bezierCurveTo(from.x + 96 * view.zoom, from.y, to.x - 96 * view.zoom, to.y, to.x, to.y);
    ctx.strokeStyle = "rgba(1,6,12,.86)";
    ctx.lineWidth = control ? Math.max(7, 10 * view.zoom) : Math.max(5, 7 * view.zoom);
    ctx.shadowBlur = 0;
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.bezierCurveTo(from.x + 96 * view.zoom, from.y, to.x - 96 * view.zoom, to.y, to.x, to.y);
    ctx.strokeStyle = color;
    ctx.lineWidth = activeWire ? Math.max(4, 7 * view.zoom) : control ? Math.max(3, 5 * view.zoom) : Math.max(2.4, 3.6 * view.zoom);
    ctx.shadowColor = activeWire ? "rgba(250,204,21,.72)" : hexToRgba(color, control || selectedWire ? .62 : .34);
    ctx.shadowBlur = activeWire ? 18 : control ? 13 : 8;
    ctx.stroke();
    ctx.shadowBlur = 0;
    if (control || selectedWire || activeWire) drawWireArrow(from, to, color, control);
  }

  function drawNode(graph, node, inlineByNode, recordHit = true) {
    const size = nodeSize(graph, node);
    const w = size.w * view.zoom, h = size.h * view.zoom;
    const x = sx(nodeX(node)), y = sy(nodeY(node));
    const selected = selectedNodeIds.has(node.node_id);
    const active = debugOverlay && debugOverlay.active_node_id === node.node_id;
    const searchHit = (searchState.spans || []).some((span) => spansOverlap(node.source_span, span));
    const breakpoint = nodeBreakpoint(node);
    const style = nodeStyle(node, graph);
    const headerH = Math.min(48, size.h - 20) * view.zoom;

    ctx.shadowColor = active ? "rgba(250,204,21,.58)" : selected ? hexToRgba(style.accent, .42) : searchHit ? "rgba(192,132,252,.42)" : "rgba(0,0,0,.45)";
    ctx.shadowBlur = active ? 34 : selected ? 26 : searchHit ? 22 : 16;
    ctx.shadowOffsetY = 12;
    roundRect(x, y, w, h, 6 * view.zoom);
    ctx.fillStyle = style.fill;
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    ctx.strokeStyle = active ? "#facc15" : selected ? style.accent : searchHit ? "#c084fc" : "#3b4657";
    ctx.lineWidth = active ? 3 : selected ? 2.2 : 1.2;
    ctx.stroke();

    roundRect(x, y, w, headerH, 6 * view.zoom);
    const headerGrad = ctx.createLinearGradient(x, y, x + w, y);
    headerGrad.addColorStop(0, style.header);
    headerGrad.addColorStop(.68, "#101722");
    headerGrad.addColorStop(1, hexToRgba(style.accent, .20));
    ctx.fillStyle = headerGrad;
    ctx.fill();
    ctx.fillStyle = style.accent;
    ctx.fillRect(x, y + headerH - 3 * view.zoom, w, 3 * view.zoom);
    ctx.fillStyle = hexToRgba(style.accent, .28);
    ctx.fillRect(x, y, 4 * view.zoom, h);

    ctx.fillStyle = hexToRgba(style.accent, .18);
    roundRect(x + 12 * view.zoom, y + 10 * view.zoom, 42 * view.zoom, 22 * view.zoom, 4 * view.zoom);
    ctx.fill();
    ctx.fillStyle = style.accent;
    ctx.font = `${Math.max(8, 10.5 * view.zoom)}px ui-monospace, Consolas, monospace`;
    ctx.textAlign = "center";
    ctx.fillText(style.glyph, x + 33 * view.zoom, y + 25 * view.zoom);
    ctx.textAlign = "left";

    ctx.fillStyle = "#f8fbff";
    ctx.font = `${Math.max(12, 14.5 * view.zoom)}px "Segoe UI", system-ui, sans-serif`;
    ctx.fillText(clipText(node.title, 24), x + 64 * view.zoom, y + 21 * view.zoom);
    ctx.fillStyle = "#9ab0cd";
    ctx.font = `${Math.max(9, 10.5 * view.zoom)}px ui-monospace, Consolas, monospace`;
    ctx.fillText(clipText(nodeSubtitle(node), 25), x + 64 * view.zoom, y + 37 * view.zoom);

    if (shouldDrawNodeBadge(node)) {
      const badgeText = nodeKindLabel(node, graph).toUpperCase();
      ctx.font = `${Math.max(7.5, 9.2 * view.zoom)}px ui-monospace, Consolas, monospace`;
      const badgeW = Math.min(118 * view.zoom, ctx.measureText(badgeText).width + 14 * view.zoom);
      const badgeY = y + headerH + 9 * view.zoom;
      roundRect(x + 14 * view.zoom, badgeY, badgeW, 20 * view.zoom, 4 * view.zoom);
      ctx.fillStyle = hexToRgba(style.accent, .14);
      ctx.fill();
      ctx.strokeStyle = hexToRgba(style.accent, .42);
      ctx.lineWidth = Math.max(.8, view.zoom);
      ctx.stroke();
      ctx.fillStyle = style.accent;
      ctx.textAlign = "center";
      ctx.fillText(clipText(badgeText, 16), x + 14 * view.zoom + badgeW / 2, badgeY + 13.5 * view.zoom);
      ctx.textAlign = "left";
    }

    if (breakpoint) {
      ctx.beginPath();
      ctx.arc(x + w - 17 * view.zoom, y + 17 * view.zoom, 6 * view.zoom, 0, Math.PI * 2);
      ctx.fillStyle = "#ef4444";
      ctx.fill();
    }

    const execIn = pinsForNode(graph, node, "input", true);
    const execOut = pinsForNode(graph, node, "output", true);
    const execTop = Math.min(72, size.h - 58);
    execIn.forEach((p, i) => drawSocketRow(p, x, y + (execTop + i * 28) * view.zoom, w, "input", recordHit));
    execOut.forEach((p, i) => drawSocketRow(p, x, y + (execTop + i * 28) * view.zoom, w, "output", recordHit));

    const inputs = pinsForNode(graph, node, "input", false);
    const outputs = pinsForNode(graph, node, "output", false);
    const execRows = Math.max(execIn.length, execOut.length, 1);
    const dataTop = execTop + execRows * 28 + 20;
    if (inputs.length || outputs.length) {
      ctx.beginPath();
      ctx.moveTo(x + 12 * view.zoom, y + (dataTop - 16) * view.zoom);
      ctx.lineTo(x + w - 12 * view.zoom, y + (dataTop - 16) * view.zoom);
      ctx.strokeStyle = "rgba(94,119,150,.28)";
      ctx.lineWidth = Math.max(.8, view.zoom);
      ctx.stroke();
    }
    inputs.forEach((p, i) => drawSocketRow(p, x, y + (dataTop + i * 30) * view.zoom, w, "input", recordHit));
    outputs.forEach((p, i) => drawSocketRow(p, x, y + (dataTop + i * 30) * view.zoom, w, "output", recordHit));

    const inline = (selected || view.zoom >= .95 ? (inlineByNode.get(node.node_id) || []) : []).slice(0, 2);
    inline.forEach((expr, i) => {
      const cy = y + (dataTop + Math.max(inputs.length, outputs.length) * 30 + 12 + i * 24) * view.zoom;
      roundRect(x + 12 * view.zoom, cy - 13 * view.zoom, w - 24 * view.zoom, 18 * view.zoom, 5 * view.zoom);
      ctx.fillStyle = "rgba(246,211,101,.11)";
      ctx.fill();
      ctx.fillStyle = "#f6d365";
      ctx.font = `${Math.max(9, 11 * view.zoom)}px ui-monospace, Consolas, monospace`;
      ctx.fillText(clipText(expr.source, 34), x + 19 * view.zoom, cy);
    });

    if (recordHit) hit.push({ x, y, w, h, node });
  }

  function drawGraph(doc) {
    latestDoc = doc;
    loadDebugState(doc);
    const graph = currentGraph(doc);
    if (!graph) return;
    selectedGraphId = graph.graph_id;
    window.__jetCanvasSelectedGraphId = selectedGraphId;
    if (!selectedNodeId || !graph.nodes.some((n) => n.node_id === selectedNodeId)) selectedNodeId = graph.entry_node;
    if (selectedNodeIds.size === 0 && selectedNodeId) selectedNodeIds.add(selectedNodeId);
    selectedNodeIds = new Set([...selectedNodeIds].filter((id) => graph.nodes.some((n) => n.node_id === id)));
    syncGraphPicker(doc);
    syncGraphList(doc);
    syncGraphStrip(doc);
    fit();
    const size = cssSize();
    drawGrid(size);
    hit = [];
    pinPoints = new Map();
    pinHit = [];
    graphSelect.value = selectedGraphId;
    const pins = new Map(graph.pins.map((p) => [p.pin_id, p]));
    const nodes = new Map(graph.nodes.map((n) => [n.node_id, n]));
    const inlineByNode = new Map();
    for (const expr of graph.inline_exprs || []) {
      if (!inlineByNode.has(expr.node_id)) inlineByNode.set(expr.node_id, []);
      inlineByNode.get(expr.node_id).push(expr);
    }
    reflowGraph(graph);

    drawCommentRegions(graph);

    for (const node of graph.nodes) {
      drawNode(graph, node, inlineByNode);
    }

    for (const wire of graph.wires) {
      const from = pinPoints.get(wire.from_pin);
      const to = pinPoints.get(wire.to_pin);
      if (!from || !to) continue;
      const activeWire = debugOverlay && debugOverlay.active_wire_id === wire.wire_id;
      const selectedWire = selectedNodeIds.has(from.pin.node_id) || selectedNodeIds.has(to.pin.node_id);
      drawWire(wire, from, to, activeWire, selectedWire);
      if (activeWire || selectedWire || view.zoom >= 1.05) drawWireTypeBadge(wire, from, to);
    }

    for (const node of graph.nodes) {
      drawNode(graph, node, inlineByNode, false);
    }

    if (drag && drag.mode === "pin") {
      drawCompatibleDropTargets(graph, drag.pin);
      const from = pinPoints.get(drag.pin.pin_id);
      if (from) {
        const plan = connectionPlan(graph, drag.pin, hoverPin);
        syncWireStatus({ title: plan.ok ? "Wire preview" : "Wire refused", detail: plan.label, color: plan.color });
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.bezierCurveTo(from.x + 90 * view.zoom, from.y, drag.mx - 90 * view.zoom, drag.my, drag.mx, drag.my);
        ctx.strokeStyle = plan.color;
        ctx.lineWidth = Math.max(2.5, 4 * view.zoom);
        ctx.setLineDash(plan.ok ? [12, 6] : [4, 6]);
        ctx.stroke();
        ctx.setLineDash([]);
        drawConnectionBadge(plan, drag.mx, drag.my);
      }
    }

    if (pendingPin && (!drag || drag.mode !== "pin")) {
      syncWireStatus({ title: "Destination needed", detail: pinName(pendingPin) + " : " + exactPinType(pendingPin), color: colorForType(pendingPin.type || "Value") });
      drawCompatibleDropTargets(graph, pendingPin);
      const from = pinPoints.get(pendingPin.pin_id);
      if (from) {
        ctx.beginPath();
        ctx.arc(from.x, from.y, Math.max(17, 22 * view.zoom), 0, Math.PI * 2);
        ctx.strokeStyle = "#7dd3fc";
        ctx.lineWidth = Math.max(1.5, 2.4 * view.zoom);
        ctx.setLineDash([8, 5]);
        ctx.stroke();
        ctx.setLineDash([]);
        drawConnectionBadge({ ok: true, label: "Select destination socket", color: "#7dd3fc" }, from.x + 36 * view.zoom, from.y - 12 * view.zoom);
      }
    }

    if (drag && drag.mode === "marquee") {
      const x = Math.min(drag.x, drag.mx), y = Math.min(drag.y, drag.my);
      const w = Math.abs(drag.mx - drag.x), h = Math.abs(drag.my - drag.y);
      ctx.setLineDash([6, 5]);
      ctx.strokeStyle = "#67e8f9";
      ctx.lineWidth = 1.5;
      ctx.strokeRect(x, y, w, h);
      ctx.fillStyle = "rgba(103,232,249,.08)";
      ctx.fillRect(x, y, w, h);
      ctx.setLineDash([]);
    }

    if (hoverPin && (!drag || drag.mode !== "pin")) drawPinHoverTooltip(hoverPin);
    drawMinimap(graph);
    drawTypeLegend(size);
    if (!pendingPin && (!drag || drag.mode !== "pin")) syncWireStatus(null);
    const selectedNode = nodes.get(selectedNodeId);
    updateDetails(graph, selectedNode, graph.pins.filter((p) => p.node_id === selectedNodeId), inlineByNode.get(selectedNodeId) || []);
    syncGraphOverview(graph, selectedNode);
    window.__jetCanvasNonblankPixels = graph.nodes.length > 0 ? 1 : 0;
    window.__jetCanvasPendingPin = pendingPin ? { pin_id: pendingPin.pin_id, name: pendingPin.name, type: pendingPin.type, direction: pendingPin.direction } : null;
    const hitMap = {
      graph_id: graph.graph_id,
      nodes: hit.map((h) => ({ node_id: h.node.node_id, title: h.node.title, kind: h.node.kind, x: h.x, y: h.y, w: h.w, h: h.h })),
      pins: pinHit.map((h) => ({ pin_id: h.pin.pin_id, node_id: h.pin.node_id, name: h.pin.name, type: h.pin.type, direction: h.pin.direction, x: h.x, y: h.y, w: h.w, h: h.h, cx: h.cx, cy: h.cy }))
    };
    window.__jetCanvasHitMap = hitMap;
    canvas.dataset.hitMap = JSON.stringify(hitMap);
    updateGraphNav(graph);
    const rails = (graph.rails && graph.rails.kinds ? graph.rails.kinds.join(", ") : "data");
    graphMeta.textContent = graph.nodes.length + " nodes / " + graph.wires.length + " wires / " + rails;
    zoomLabel.textContent = Math.round(view.zoom * 100) + "%";
    sourceId.textContent = doc.source_id || "source";
    revision.textContent = (doc.revision || "").slice(0, 18);
  }

  graphSelect.addEventListener("change", function () {
    switchGraph(graphSelect.value);
  });

  function drawMinimap(graph) {
    mini.clearRect(0, 0, minimap.width, minimap.height);
    mini.fillStyle = "#07101c";
    mini.fillRect(0, 0, minimap.width, minimap.height);
    const b = graphBounds(graph);
    const scale = Math.min((minimap.width - 20) / Math.max(1, b.maxX - b.minX), (minimap.height - 20) / Math.max(1, b.maxY - b.minY));
    for (const n of graph.nodes) {
      const size = nodeSize(graph, n);
      mini.fillStyle = n.node_id === selectedNodeId ? "#38bdf8" : "#31557b";
      mini.fillRect(10 + (nodeX(n) - b.minX) * scale, 10 + (nodeY(n) - b.minY) * scale, Math.max(16, size.w * scale), Math.max(9, size.h * scale));
    }
  }

  function nodesInRegion(graph, region) {
    return (graph.nodes || []).filter((node) => spansOverlap(node.source_span, region.source_span));
  }

  function commentRegionBounds(graph, region) {
    const b = region.bounds || {};
    if (b.w > 0 && b.h > 0) return { x: b.x || 0, y: b.y || 0, w: b.w, h: b.h };
    const nodes = nodesInRegion(graph, region);
    if (nodes.length === 0) return { x: 120, y: 120, w: 360, h: 180 };
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const node of nodes) {
      const size = nodeSize(graph, node);
      minX = Math.min(minX, nodeX(node));
      minY = Math.min(minY, nodeY(node));
      maxX = Math.max(maxX, nodeX(node) + size.w);
      maxY = Math.max(maxY, nodeY(node) + size.h);
    }
    return { x: minX - 26, y: minY - 36, w: maxX - minX + 52, h: maxY - minY + 70 };
  }

  function drawCommentRegions(graph) {
    for (const region of (graph.regions || []).filter((r) => r.kind === "comment")) {
      const b = commentRegionBounds(graph, region);
      const x = sx(b.x), y = sy(b.y), w = b.w * view.zoom, h = b.h * view.zoom;
      roundRect(x, y, w, h, 7);
      ctx.fillStyle = hexToRgba(region.color, region.alpha);
      ctx.fill();
      ctx.strokeStyle = region.color || "#2563eb";
      ctx.lineWidth = 1.5;
      ctx.setLineDash([8, 5]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = "#eaf3ff";
      ctx.font = `${Math.max(11, 14 * view.zoom)}px "Segoe UI", system-ui, sans-serif`;
      ctx.fillText(region.title || "Comment", x + 12 * view.zoom, y + 23 * view.zoom);
    }
  }

  function regionsForNode(graph, node) {
    if (!node) return [];
    return (graph.regions || []).filter((region) => region.kind === "comment" && spansOverlap(region.source_span, node.source_span));
  }

  function debugRows(items) {
    return (items || []).map((item) => `<div class="pin-row"><b>${escapeHtml(item.name || "frame")}</b><br><span class="tag">${escapeHtml(item.value || String(item))}</span></div>`).join("");
  }

  function signatureFromVisibleFunctionPins(fnMeta, nodeTitle, retOverride) {
    const retType = retOverride !== undefined ? retOverride : (document.getElementById("function-return-type") && document.getElementById("function-return-type").value.trim()) || "Void";
    const ret = retType && retType !== "Void" ? " -> " + retType : "";
    const rows = [...details.querySelectorAll("[data-fn-param]")];
    const params = rows.map((row) => {
      const name = (row.querySelector("[data-param-name]") || {}).value || "";
      const type = (row.querySelector("[data-param-type]") || {}).value || "Int";
      const fallback = (row.querySelector("[data-param-default]") || {}).value || "";
      const defaultExpr = fallback.trim() ? " = " + fallback.trim() : "";
      return name.trim() + ": " + type.trim() + defaultExpr;
    }).filter((p) => !p.startsWith(":"));
    const visibility = fnMeta && fnMeta.visibility === "public" ? "pub " : "";
    return visibility + "fn " + ((fnMeta && fnMeta.name) || nodeTitle || "function") + "(" + params.join(", ") + ")" + ret;
  }

  function applyFunctionPins(graph, fnMeta, nodeTitle, retOverride) {
    const signature = signatureFromVisibleFunctionPins(fnMeta, nodeTitle, retOverride);
    window.__jetCanvasLastSignature = signature;
    postTransaction({ schema_version: 1, op: "edit_function_signature", revision: latestDoc.revision, graph_id: graph.graph_id, signature });
    return signature;
  }

  function nextParamName(fnMeta) {
    const used = new Set((fnMeta && fnMeta.params || []).map((p) => p.name || ""));
    let i = 1;
    while (used.has("input" + i)) i += 1;
    return "input" + i;
  }

  function pinPortHtml(type) {
    return `<span class="pin-port" style="color:${escapeAttr(colorForType(type))}"></span>`;
  }

  function typeChipHtml(type) {
    return `<span class="type-chip" style="color:${escapeAttr(colorForType(type))}">${escapeHtml(type || "Value")}</span>`;
  }

  function functionParamRow(p, i) {
    const pinType = p && p.type ? p.type : "Int";
    const pinName = p && p.name ? p.name : "value";
    const defaultSource = p && p.default_source ? p.default_source : "";
    const defaultLabel = defaultSource ? "default " + defaultSource : "required";
    return `<div class="pin-editor-row" data-fn-param="${escapeAttr(i)}"><div class="pin-editor-title">${pinPortHtml(pinType)}<b>${escapeHtml(pinName)}</b>${typeChipHtml(pinType)}</div><div class="lane-meta">${escapeHtml(defaultLabel)}</div><div class="pin-tools"><input data-param-name="${escapeAttr(i)}" aria-label="Input pin name" title="Input pin name" value="${escapeAttr(pinName)}"><input data-param-type="${escapeAttr(i)}" aria-label="Input pin type" title="Input pin type" value="${escapeAttr(pinType)}"><input data-param-default="${escapeAttr(i)}" aria-label="Default expression" title="Default expression" placeholder="default" value="${escapeAttr(defaultSource)}"><button data-param-remove="${escapeAttr(i)}" title="Remove input pin">-</button></div></div>`;
  }

  function functionReturnRow(retType) {
    const outType = retType || "Void";
    const color = colorForType(outType);
    return `<div class="pin-editor-row"><div class="pin-editor-title"><span id="function-return-port" class="pin-port" style="color:${escapeAttr(color)}"></span><b>return</b><span id="function-return-type-chip" class="type-chip" style="color:${escapeAttr(color)}">${escapeHtml(outType)}</span></div><div id="function-return-meta" class="lane-meta">${outType === "Void" ? "no output value" : "output pin"}</div><div class="pin-tools output-pin-tools"><input id="function-return-type" aria-label="Return type" title="Return type" value="${escapeAttr(outType)}"><button id="set-function-output">Set</button><button id="remove-function-output">Void</button></div></div>`;
  }

  function syncReturnEditorPreview(type) {
    const outType = type && type.trim() ? type.trim() : "Void";
    const color = colorForType(outType);
    const port = document.getElementById("function-return-port");
    const chip = document.getElementById("function-return-type-chip");
    const meta = document.getElementById("function-return-meta");
    if (port) port.style.color = color;
    if (chip) {
      chip.style.color = color;
      chip.textContent = outType;
    }
    if (meta) meta.textContent = outType === "Void" ? "no output value" : "output pin";
  }

  function pinCardHtml(p) {
    const type = p.type || "Value";
    const color = colorForType(type);
    const rail = pinRail(p);
    const flags = [p.direction, rail, p.fallible ? "fallible" : "", p.effect_grant_need ? "effect" : ""].filter(Boolean).join(" / ");
    return `<div class="pin-card" style="--pin-color:${escapeAttr(color)}">${pinPortHtml(type)}<div class="pin-card-title"><b>${escapeHtml(p.name)}</b><small>${escapeHtml(flags)} - ${escapeHtml(type)}</small></div><button data-pin-menu="${escapeAttr(p.pin_id)}">Actions</button></div>`;
  }

  function updateDetails(graph, node, pins, inline) {
    if (!node) {
      details.innerHTML = "<div class=\"details-empty\"><b>No node selected</b><span class=\"tag\">Select, marquee, or use the graph tabs.</span></div>";
      return;
    }
    const style = nodeStyle(node, graph);
    details.style.setProperty("--node-accent", style.accent);
    const span = node.source_span || { start: 0, end: 0 };
    const pinRows = pins.map(pinCardHtml).join("");
    const inlineRows = inline.map((expr) => `<div class="inline-row"><b>${escapeHtml(expr.role)}</b><code>${escapeHtml(expr.source)}</code><div class="edit-grid"><input data-inline-id="${escapeHtml(expr.inline_expr_id)}" value="${escapeAttr(expr.source)}"><button data-inline-apply="${escapeHtml(expr.inline_expr_id)}">Apply expression</button><button data-inline-promote="${escapeHtml(expr.inline_expr_id)}">Promote to binding</button><button data-inline-convert="${escapeHtml(expr.inline_expr_id)}">Insert conversion</button><button data-inline-preview-extract="${escapeHtml(expr.inline_expr_id)}">Preview extract</button><button data-inline-extract="${escapeHtml(expr.inline_expr_id)}">Extract function</button></div></div>`).join("");
    const rename = node.kind === "binding" ? `<div class="edit-grid"><label>Rename binding<input id="rename-to" value="${escapeAttr(node.title)}"></label><button id="preview-rename">Preview rename</button><button id="rename-binding" class="primary">Rename</button></div>` : "";
    const calleeGraph = (node.kind === "call" || node.kind === "method") ? graphForFunctionName(node.title) : null;
    const calleeOpen = calleeGraph ? `<button id="open-callee-graph">Open ${escapeHtml(calleeGraph.title)} graph</button>` : "";
    const fnMeta = node.node_id === graph.entry_node ? graph.function : null;
    const fnReturnType = fnMeta ? (typeof fnMeta.returns === "string" ? fnMeta.returns : (fnMeta.returns && fnMeta.returns.type) || "Void") : "Void";
    const fnParams = fnMeta ? (fnMeta.params || []).map((p, i) => functionParamRow(p, i)).join("") : "";
    const fnReturnPanel = fnMeta ? functionReturnRow(fnReturnType) : "";
    const fnEvents = fnMeta ? (graph.event_views || []).map((event) => `<div class="pin-row"><b>${escapeHtml(event.title || event.function)}</b><br><span class="tag">${escapeHtml(event.semantics || "ordinary_jet_function")}</span></div>`).join("") : "";
    const fnPanel = fnMeta ? `<h2>Function</h2><div class="signature-board"><div class="signature-head"><div><span class="sig-eyebrow">function graph</span><b>${escapeHtml(fnMeta.visibility || "private")} ${escapeHtml(fnMeta.name || node.title)}</b><code>${escapeHtml(fnMeta.signature || "")}</code></div><button id="create-function" title="Create sibling function">New</button></div><div class="pin-lane"><div class="lane-head"><b>Inputs</b><span class="lane-meta">${(fnMeta.params || []).length} pins</span><button id="add-function-pin">+ Input</button></div><div class="pin-list" id="function-pin-list">${fnParams || "<div class=\"pin-empty\">No input pins</div>"}</div></div><div class="pin-lane"><div class="lane-head"><b>Output</b><span class="lane-meta">return type</span><button id="add-function-output">+ Output</button></div><div class="pin-list">${fnReturnPanel}</div></div><div class="signature-source"><span class="sig-eyebrow">source signature</span><code>${escapeHtml(fnMeta.signature || "")}</code><input id="function-signature" value="${escapeAttr(fnMeta.signature || "")}"><div class="rename-strip"><input id="function-rename-to" aria-label="Function name" title="Function name" value="${escapeAttr(fnMeta.name || node.title)}"><button id="rename-function">Rename</button></div></div><div class="signature-actions"><button id="edit-function-signature">Apply signature</button><button id="apply-function-pins" class="primary">Apply pins</button></div></div>${fnEvents ? `<h2>Callback views</h2><div class="pin-list">${fnEvents}</div>` : ""}` : "";
    const bpLabel = nodeBreakpoint(node) ? "Remove breakpoint" : "Set breakpoint";
    const locals = debugRows(debugOverlay && debugOverlay.locals);
    const watches = debugRows(debugOverlay && debugOverlay.watches);
    const stack = (debugOverlay && debugOverlay.call_stack || []).map((frame) => `<div class="pin-row"><span class="tag">${escapeHtml(frame)}</span></div>`).join("");
    const regionRows = regionsForNode(graph, node).map((region) => {
      const b = region.bounds || { x: 0, y: 0, w: 360, h: 180 };
      const bounds = [b.x || 0, b.y || 0, b.w || 360, b.h || 180].join(",");
      return `<div class="inline-row"><b>${escapeHtml(region.title || "Comment")}</b><code>${escapeHtml(region.region_id)}</code><div class="edit-grid"><input data-region-title="${escapeHtml(region.region_id)}" value="${escapeAttr(region.title || "Comment")}"><input data-region-color="${escapeHtml(region.region_id)}" value="${escapeAttr(region.color || "#2563eb")}"><input data-region-alpha="${escapeHtml(region.region_id)}" value="${escapeAttr(region.alpha || "0.18")}"><input data-region-bounds="${escapeHtml(region.region_id)}" value="${escapeAttr(bounds)}"><button data-region-apply="${escapeHtml(region.region_id)}">Apply comment</button><button data-region-delete="${escapeHtml(region.region_id)}">Delete comment</button></div></div>`;
    }).join("");
    const affords = (node.edit_affordances || []).slice(0, 4).map((a) => `<span class="details-chip">${escapeHtml(a)}</span>`).join("");
    details.innerHTML = `
      <div class="details-hero">
        <div class="details-titleline"><span class="node-glyph">${escapeHtml(style.glyph)}</span><div class="details-title"><p class="title">${escapeHtml(node.title)}</p><div class="kind">${escapeHtml(nodeKindLabel(node, graph))}</div></div></div>
        <div class="details-chips dev-only"><span class="details-chip">${escapeHtml(node.kind)}</span><span class="details-chip">${pins.length} pins</span>${affords}</div>
        <div class="quick-actions"><button id="source-jump">Jump source</button><button id="find-references">Find refs</button>${calleeOpen ? calleeOpen.replace("<button", "<button class=\"wide\"") : ""}</div>
        <dl class="dev-only">
          <dt>span</dt><dd>${span.start}..${span.end}</dd>
          <dt>node</dt><dd>${escapeHtml(node.node_id)}</dd>
        </dl>
      </div>
      ${rename}
      ${fnPanel}
      <div class="dev-only">
        <h2>Debug</h2>
        <div class="edit-grid"><button id="debug-toggle-break">${bpLabel}</button><button id="debug-add-watch">Add watch</button></div>
        <div class="pin-list">${locals || watches || stack ? locals + watches + stack : "<div class=\"tag\">no live values</div>"}</div>
        <h2>Comments</h2><div class="inline-list">${regionRows || "<div class=\"tag\">none</div>"}</div>
        <h2>Pins</h2><div class="pin-list">${pinRows || "<div class=\"tag\">none</div>"}</div>
        <h2>Inline</h2><div class="inline-list">${inlineRows || "<div class=\"tag\">none</div>"}</div>
      </div>
    `;
    const renameButton = document.getElementById("rename-binding");
    if (renameButton) {
      renameButton.addEventListener("click", () => {
        const to = document.getElementById("rename-to").value.trim();
        postTransaction({ schema_version: 1, op: "rename_binding", revision: latestDoc.revision, from: node.title, to });
      });
    }
    const previewRename = document.getElementById("preview-rename");
    if (previewRename) {
      previewRename.addEventListener("click", () => {
        const to = document.getElementById("rename-to").value.trim();
        postQuery({ op: "preview_rename", symbol: node.title, to });
      });
    }
    const renameFunction = document.getElementById("rename-function");
    if (renameFunction && fnMeta) {
      renameFunction.addEventListener("click", () => {
        const to = document.getElementById("function-rename-to").value.trim();
        postTransaction({ schema_version: 1, op: "rename_function", revision: latestDoc.revision, from: fnMeta.name, to });
      });
    }
    const editFunctionSignature = document.getElementById("edit-function-signature");
    if (editFunctionSignature && fnMeta) {
      editFunctionSignature.addEventListener("click", () => {
        const signature = document.getElementById("function-signature").value.trim();
        postTransaction({ schema_version: 1, op: "edit_function_signature", revision: latestDoc.revision, graph_id: graph.graph_id, signature });
      });
    }
    const createFunction = document.getElementById("create-function");
    if (createFunction) {
      createFunction.addEventListener("click", () => {
        const name = window.prompt("Function name", "helper");
        if (!name) return;
        const params = window.prompt("Parameters", "value: Int") || "";
        const ret_type = window.prompt("Return type", "Int") || "Int";
        postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params, ret_type });
      });
    }
    const openCalleeGraph = document.getElementById("open-callee-graph");
    if (openCalleeGraph && calleeGraph) {
      openCalleeGraph.addEventListener("click", () => openFunctionGraph(calleeGraph.title));
    }
    const addFunctionPin = document.getElementById("add-function-pin");
    if (addFunctionPin && fnMeta) {
      addFunctionPin.addEventListener("click", () => {
        const list = document.getElementById("function-pin-list");
        const i = "new" + Date.now();
        const row = document.createElement("div");
        row.innerHTML = functionParamRow({ name: nextParamName(fnMeta), type: "Int", default_source: "" }, i);
        const editorRow = row.firstElementChild;
        if (editorRow) {
          const empty = list.querySelector(".pin-empty");
          if (empty) empty.remove();
          list.appendChild(editorRow);
          const remove = editorRow.querySelector("[data-param-remove]");
          if (remove) remove.addEventListener("click", () => {
            editorRow.remove();
            applyFunctionPins(graph, fnMeta, node.title);
          });
          applyFunctionPins(graph, fnMeta, node.title);
        }
      });
    }
    ["apply-function-pins", "set-function-output", "remove-function-output", "add-function-output"].forEach((id) => {
      const button = document.getElementById(id);
      if (button) button.addEventListener("click", handleFunctionPinButton);
    });
    details.querySelectorAll("[data-param-remove]").forEach((button) => {
      button.addEventListener("click", () => {
        const row = button.closest("[data-fn-param]");
        if (row) row.remove();
        if (fnMeta) applyFunctionPins(graph, fnMeta, node.title);
      });
    });
    details.querySelectorAll("[data-pin-menu]").forEach((button) => {
      button.addEventListener("click", (ev) => {
        const pin = pins.find((p) => p.pin_id === button.getAttribute("data-pin-menu"));
        if (pin) openPinMenu(pin, ev.clientX, ev.clientY);
      });
    });
    const sourceJump = document.getElementById("source-jump");
    if (sourceJump) {
      sourceJump.addEventListener("click", () => {
        setSourceHash(span);
        showToast("Source span copied to URL");
      });
    }
    const findReferences = document.getElementById("find-references");
    if (findReferences) {
      findReferences.addEventListener("click", () => postQuery({ op: "references", symbol: node.title }));
    }
    details.querySelectorAll("[data-inline-apply]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-apply");
        const input = details.querySelector(`[data-inline-id="${cssEscape(id)}"]`);
        postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: id, new_expr: input.value });
      });
    });
    details.querySelectorAll("[data-inline-promote]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-promote");
        const name = window.prompt("Binding name", "value");
        if (name) postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: id, name });
      });
    });
    details.querySelectorAll("[data-inline-convert]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-convert");
        const callee = window.prompt("Conversion function", "Float.from");
        if (callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: id, callee });
      });
    });
    details.querySelectorAll("[data-inline-preview-extract], [data-inline-extract]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-preview-extract") || button.getAttribute("data-inline-extract");
        const name = window.prompt("Helper function", "extracted");
        if (!name) return;
        const op = button.hasAttribute("data-inline-preview-extract") ? "preview_extract_inline_expr" : "extract_inline_expr";
        postTransaction({ schema_version: 1, op, revision: latestDoc.revision, inline_expr_id: id, function: name, ret_type: "Int" });
      });
    });
    const toggle = document.getElementById("debug-toggle-break");
    if (toggle) toggle.addEventListener("click", () => toggleBreakpoint(node));
    const watch = document.getElementById("debug-add-watch");
    if (watch) watch.addEventListener("click", () => {
      const name = window.prompt("Watch local", node.title);
      addWatch(name && name.trim());
    });
    details.querySelectorAll("[data-region-apply]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-region-apply");
        postTransaction({
          schema_version: 1,
          op: "edit_comment_region",
          revision: latestDoc.revision,
          region_id: id,
          title: details.querySelector(`[data-region-title="${cssEscape(id)}"]`).value,
          color: details.querySelector(`[data-region-color="${cssEscape(id)}"]`).value,
          alpha: details.querySelector(`[data-region-alpha="${cssEscape(id)}"]`).value,
          bounds: details.querySelector(`[data-region-bounds="${cssEscape(id)}"]`).value
        });
      });
    });
    details.querySelectorAll("[data-region-delete]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-region-delete");
        postTransaction({ schema_version: 1, op: "delete_comment_region", revision: latestDoc.revision, region_id: id });
      });
    });
  }

  function selectNode(node, mode) {
    if (mode === "toggle") {
      if (selectedNodeIds.has(node.node_id)) selectedNodeIds.delete(node.node_id);
      else selectedNodeIds.add(node.node_id);
    } else if (mode === "add") {
      selectedNodeIds.add(node.node_id);
    } else {
      selectedNodeIds = new Set([node.node_id]);
    }
    selectedNodeId = node.node_id;
    const s = node.source_span || { start: 0, end: 0 };
    setSourceHash(s);
    if (latestDoc) drawGraph(latestDoc);
  }

  function hitNodeAt(x, y) {
    for (let i = hit.length - 1; i >= 0; i--) {
      const h = hit[i];
      if (x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h) return h;
    }
    return null;
  }

  function hitPinAt(x, y) {
    let best = null;
    let bestDistance = Infinity;
    for (let i = pinHit.length - 1; i >= 0; i--) {
      const h = pinHit[i];
      if (x < h.x || x > h.x + h.w || y < h.y || y > h.y + h.h) continue;
      const dx = x - (h.cx || h.x + h.w / 2);
      const dy = y - (h.cy || h.y + h.h / 2);
      const distance = dx * dx + dy * dy;
      if (distance < bestDistance) {
        best = h.pin;
        bestDistance = distance;
      }
    }
    return best;
  }

  function numericType(type) {
    return ["Int", "Float", "F32", "F64"].includes(type || "");
  }

  function compatiblePin(from, to) {
    if (!from || !to || from.pin_id === to.pin_id) return false;
    if (from.direction === to.direction) return false;
    const out = from.direction === "output" ? from : to;
    const input = from.direction === "input" ? from : to;
    if (out.type === input.type) return true;
    return numericType(out.type) && numericType(input.type);
  }

  function connectionPlan(graph, fromPin, toPin) {
    if (!fromPin) return { ok: false, label: "No pin", color: "#fb7185" };
    if (!toPin) return { ok: true, label: "Release for actions", color: "#7dd3fc" };
    if (fromPin.pin_id === toPin.pin_id) return { ok: false, label: "Same socket", color: "#fb7185" };
    if (fromPin.direction === toPin.direction) {
      const expected = fromPin.direction === "output" ? "Drop on an input socket" : "Drop on an output socket";
      return { ok: false, label: expected, color: "#fb7185" };
    }
    const out = fromPin.direction === "output" ? fromPin : toPin;
    const input = fromPin.direction === "input" ? fromPin : toPin;
    if (!compatiblePin(fromPin, toPin)) return { ok: false, label: "Type mismatch " + (out.type || "?") + " -> " + (input.type || "?"), color: "#fb7185" };
    if (!exactPinMatch(fromPin, toPin)) return { ok: true, label: "Insert visible conversion", color: "#f59e0b" };
    const wire = wireIntoPin(graph, input);
    const replacement = sourceExprForOutputPin(out);
    if (wire && replacement) return { ok: true, label: "Rewire source", color: "#a7f3d0" };
    return { ok: true, label: "Compatible preview", color: "#fde68a" };
  }

  function drawCompatibleDropTargets(graph, fromPin) {
    const seen = new Set();
    for (const hit of pinHit) {
      const pin = hit.pin;
      if (!compatiblePin(fromPin, pin) || seen.has(pin.pin_id)) continue;
      seen.add(pin.pin_id);
      const point = pinPoints.get(pin.pin_id);
      if (!point) continue;
      ctx.beginPath();
      ctx.arc(point.x, point.y, Math.max(13, 18 * view.zoom), 0, Math.PI * 2);
      ctx.strokeStyle = exactPinMatch(fromPin, pin) ? "#a7f3d0" : "#f59e0b";
      ctx.lineWidth = Math.max(1.4, 2.2 * view.zoom);
      ctx.shadowColor = exactPinMatch(fromPin, pin) ? "rgba(167,243,208,.52)" : "rgba(245,158,11,.44)";
      ctx.shadowBlur = 10;
      ctx.stroke();
      ctx.shadowBlur = 0;
    }
  }

  function drawConnectionBadge(plan, x, y) {
    const text = plan.label || "";
    ctx.font = `${Math.max(10, 12 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 9 * view.zoom;
    const badgeW = Math.min(230 * view.zoom, ctx.measureText(text).width + padX * 2);
    const badgeH = 26 * view.zoom;
    const bx = Math.min(x + 14 * view.zoom, cssSize().width - badgeW - 8);
    const by = Math.max(8, y - badgeH - 12 * view.zoom);
    roundRect(bx, by, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.94)";
    ctx.fill();
    ctx.strokeStyle = plan.color || "#7dd3fc";
    ctx.lineWidth = Math.max(1, view.zoom);
    ctx.stroke();
    ctx.fillStyle = plan.color || "#dbeafe";
    ctx.fillText(clipText(text, 28), bx + padX, by + 17 * view.zoom);
  }

  function drawWireTypeBadge(wire, from, to) {
    const label = wire.wire_kind === "control" ? "EXEC" : (from.pin && from.pin.type) || wire.wire_kind || "Value";
    const color = wireColor(wire, from);
    const mx = (from.x + to.x) / 2;
    const my = (from.y + to.y) / 2;
    ctx.font = `${Math.max(8, 10 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 7 * view.zoom;
    const badgeW = Math.min(112 * view.zoom, ctx.measureText(label).width + padX * 2);
    const badgeH = 19 * view.zoom;
    roundRect(mx - badgeW / 2, my - badgeH / 2, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.90)";
    ctx.fill();
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(.8, view.zoom);
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.fillText(clipText(label, 16), mx, my + 3.5 * view.zoom);
    ctx.textAlign = "left";
  }

  function exactPinMatch(from, to) {
    if (!from || !to) return false;
    const out = from.direction === "output" ? from : to;
    const input = from.direction === "input" ? from : to;
    return out.type === input.type;
  }

  function sourceExprForOutputPin(pin) {
    if (!pin || pin.direction !== "output") return null;
    const name = pin.name || "";
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !["value", "result", "ok", "target"].includes(name)) return name;
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    const node = graph && (graph.nodes || []).find((n) => n.node_id === pin.node_id);
    if (node && /^[A-Za-z_][A-Za-z0-9_]*$/.test(node.title) && node.kind === "binding") return node.title;
    return null;
  }

  function wireIntoPin(graph, pin) {
    if (!graph || !pin) return null;
    return (graph.wires || []).find((w) => w.to_pin === pin.pin_id && w.source_span);
  }

  function inlineForPin(graph, pin) {
    if (!graph || !pin || !pin.source_span) return null;
    return (graph.inline_exprs || []).find((e) => e.source_span && spansOverlap(e.source_span, pin.source_span));
  }

  function selectMarquee() {
    if (!drag || drag.mode !== "marquee") return;
    const x0 = Math.min(drag.x, drag.mx), x1 = Math.max(drag.x, drag.mx);
    const y0 = Math.min(drag.y, drag.my), y1 = Math.max(drag.y, drag.my);
    const next = drag.additive ? new Set(selectedNodeIds) : new Set();
    for (const h of hit) {
      if (h.x < x1 && h.x + h.w > x0 && h.y < y1 && h.y + h.h > y0) next.add(h.node.node_id);
    }
    selectedNodeIds = next;
    selectedNodeId = [...selectedNodeIds][0] || selectedNodeId;
  }

  function completeConnection(fromPin, target, graph) {
    const plan = connectionPlan(graph, fromPin, target);
    window.__jetCanvasLastConnectionPlan = plan;
    if (compatiblePin(fromPin, target)) {
      const out = fromPin.direction === "output" ? fromPin : target;
      const input = fromPin.direction === "input" ? fromPin : target;
      const wire = wireIntoPin(graph, input);
      const replacement = sourceExprForOutputPin(out);
      if (exactPinMatch(fromPin, target) && wire && replacement) {
        postTransaction({ schema_version: 1, op: "move_link", revision: latestDoc.revision, wire_id: wire.wire_id, replacement });
      } else if (!exactPinMatch(fromPin, target)) {
        const expr = inlineForPin(graph, input);
        const callee = window.prompt("Visible conversion function", (input.type || "Value") + ".from");
        if (expr && callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, callee });
        else showToast("Conversion needs an inline source expression");
      } else {
        showToast(plan.label + ": no safe source anchor");
      }
      return true;
    }
    if (target) showToast("Wire refused: " + plan.label);
    return false;
  }

  canvas.addEventListener("click", function (ev) {
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    if (hitPinAt(x, y)) {
      ev.preventDefault();
      return;
    }
    const found = hitNodeAt(x, y);
    if (found) selectNode(found.node, ev.ctrlKey || ev.metaKey ? "toggle" : ev.shiftKey ? "add" : "replace");
  });

  canvas.addEventListener("dblclick", function (ev) {
    const rect = canvas.getBoundingClientRect();
    const found = hitNodeAt(ev.clientX - rect.left, ev.clientY - rect.top);
    if (found && openFunctionGraph(found.node.title)) ev.preventDefault();
  });

  canvas.addEventListener("mousedown", function (ev) {
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const pin = hitPinAt(x, y);
    if (pin) {
      hoverPin = pin;
      drag = { mode: "pin", pin, x, y, mx: x, my: y };
      showToast(pin.name + ": " + pin.type);
      return;
    }
    setPendingPin(null);
    const found = hitNodeAt(x, y);
    if (found) {
      selectNode(found.node, ev.ctrlKey || ev.metaKey ? "toggle" : ev.shiftKey ? "add" : "replace");
      const starts = new Map();
      for (const id of selectedNodeIds) starts.set(id, nodeOffsets.get(id) || { x: 0, y: 0 });
      drag = { mode: "node", x, y, wx: wx(x), wy: wy(y), starts };
    } else if (ev.button === 1 || ev.altKey || spaceDown) {
      drag = { mode: "pan", x, y, ox: view.x, oy: view.y };
    } else {
      drag = { mode: "marquee", x, y, mx: x, my: y, additive: ev.shiftKey || ev.ctrlKey || ev.metaKey };
    }
  });

  window.addEventListener("mousemove", function (ev) {
    const rect = canvas.getBoundingClientRect();
    if (!drag) {
      const nextHover = hitPinAt(ev.clientX - rect.left, ev.clientY - rect.top);
      if ((nextHover && !hoverPin) || (!nextHover && hoverPin) || (nextHover && hoverPin && nextHover.pin_id !== hoverPin.pin_id)) {
        hoverPin = nextHover;
        if (latestDoc) drawGraph(latestDoc);
      }
      return;
    }
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    if (drag.mode === "pan") {
      view.x = drag.ox + (x - drag.x);
      view.y = drag.oy + (y - drag.y);
    } else if (drag.mode === "node") {
      const dx = wx(x) - drag.wx;
      const dy = wy(y) - drag.wy;
      for (const [id, start] of drag.starts.entries()) nodeOffsets.set(id, { x: start.x + dx, y: start.y + dy });
    } else if (drag.mode === "marquee") {
      drag.mx = x;
      drag.my = y;
      selectMarquee();
    } else if (drag.mode === "pin") {
      drag.mx = x;
      drag.my = y;
      hoverPin = hitPinAt(x, y);
    }
    if (latestDoc) drawGraph(latestDoc);
  });

  window.addEventListener("mouseup", function (ev) {
    if (drag && drag.mode === "node") showToast("Moved " + selectedNodeIds.size + " node" + (selectedNodeIds.size === 1 ? "" : "s") + " locally");
    if (drag && drag.mode === "pin") {
      const rect = canvas.getBoundingClientRect();
      const target = hitPinAt(ev.clientX - rect.left, ev.clientY - rect.top) || hoverPin;
      const graph = latestDoc ? currentGraph(latestDoc) : null;
      const moved = Math.abs(drag.mx - drag.x) > 5 || Math.abs(drag.my - drag.y) > 5;
      if (!moved && pendingPin && target) {
        const done = completeConnection(pendingPin, target, graph);
        setPendingPin(done || pendingPin.pin_id === target.pin_id ? null : pendingPin);
      } else if (!moved && target && target.pin_id === drag.pin.pin_id) {
        if (pendingPin && pendingPin.pin_id === target.pin_id) {
          setPendingPin(null);
          showToast("Connection cancelled");
        } else {
          setPendingPin(drag.pin);
          showToast("Select destination socket");
        }
      } else if (target) {
        setPendingPin(null);
        completeConnection(drag.pin, target, graph);
      } else {
        setPendingPin(null);
        openPinMenu(drag.pin, ev.clientX, ev.clientY);
      }
    }
    drag = null;
    if (latestDoc) drawGraph(latestDoc);
  });

  canvas.addEventListener("contextmenu", function (ev) {
    ev.preventDefault();
    closeContextMenu();
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const pin = hitPinAt(x, y);
    if (pin) {
      openPinMenu(pin, ev.clientX, ev.clientY);
      return;
    }
    const found = hitNodeAt(x, y);
    if (found) {
      selectNode(found.node, "replace");
      const graph = latestDoc ? currentGraph(latestDoc) : null;
      openContextMenu(ev.clientX, ev.clientY, found.node.title, nodeContextActions(graph, found.node));
    } else {
      openGraphActionPalette(ev.clientX, ev.clientY);
    }
  });

  canvas.addEventListener("wheel", function (ev) {
    ev.preventDefault();
    closeContextMenu();
    const rect = canvas.getBoundingClientRect();
    const mx = ev.clientX - rect.left;
    const my = ev.clientY - rect.top;
    const before = { x: wx(mx), y: wy(my) };
    const factor = ev.deltaY < 0 ? 1.09 : .92;
    view.zoom = Math.max(.35, Math.min(2.2, view.zoom * factor));
    view.x = mx - before.x * view.zoom;
    view.y = my - before.y * view.zoom;
    if (latestDoc) drawGraph(latestDoc);
  }, { passive: false });

  graphBack.addEventListener("click", () => {
    if (!graphBackStack.length || !selectedGraphId) return;
    const previous = graphBackStack.pop();
    graphForwardStack.push(selectedGraphId);
    switchGraph(previous, { push: false, toast: "Back to " + graphTitle(previous) });
  });
  graphForward.addEventListener("click", () => {
    if (!graphForwardStack.length || !selectedGraphId) return;
    const next = graphForwardStack.pop();
    graphBackStack.push(selectedGraphId);
    switchGraph(next, { push: false, toast: "Forward to " + graphTitle(next) });
  });
  dockGraphs.addEventListener("click", () => setDrawer(dockGraphs.classList.contains("is-active") ? null : "graphs"));
  dockDetails.addEventListener("click", () => setDrawer(dockDetails.classList.contains("is-active") ? null : "details"));
  document.getElementById("fit").addEventListener("click", fitGraph);
  document.getElementById("reload").addEventListener("click", loadGraph);
  sourceDiff.addEventListener("click", showSourceDiff);
  viewToggle.addEventListener("click", () => setViewMode(viewMode === "source" ? "graph" : "source"));
  developerModeButton.addEventListener("click", () => setDeveloperMode(!developerMode));
  undoEdit.addEventListener("click", undoTransaction);
  redoEdit.addEventListener("click", redoTransaction);
  debugStep.addEventListener("click", () => runDebug(["s"]));
  debugNext.addEventListener("click", () => runDebug(["n"]));
  debugContinue.addEventListener("click", () => runDebug(["c"]));
  debugStop.addEventListener("click", stopDebug);
  debugBreak.addEventListener("click", () => {
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    const node = graph ? graph.nodes.find((n) => n.node_id === selectedNodeId) : null;
    if (node) toggleBreakpoint(node);
  });
  debugWatch.addEventListener("click", () => {
    const name = window.prompt("Watch local", "");
    addWatch(name && name.trim());
  });
  canvasSearch.addEventListener("input", runCanvasSearch);
  window.addEventListener("hashchange", applySourceHash);

  window.addEventListener("keydown", function (ev) {
    if (ev.key === " ") spaceDown = true;
    if (ev.key === "Escape") closeContextMenu();
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "`") {
      ev.preventDefault();
      setViewMode(viewMode === "source" ? "graph" : "source");
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && (ev.key === "k" || ev.key === "f")) {
      ev.preventDefault();
      canvasSearch.focus();
      canvasSearch.select();
      showToast("Find in Canvas");
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "p") {
      ev.preventDefault();
      const rect = canvas.getBoundingClientRect();
      openGraphActionPalette(rect.left + Math.min(rect.width - 20, 220), rect.top + 90);
      showToast("Context actions");
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "z") {
      ev.preventDefault();
      undoTransaction();
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && (ev.key === "y" || (ev.shiftKey && ev.key === "Z"))) {
      ev.preventDefault();
      redoTransaction();
      return;
    }
    if (ev.key === "/" && document.activeElement !== canvasSearch) {
      ev.preventDefault();
      canvasSearch.focus();
      return;
    }
    if (ev.key === "Escape") {
      if (compactCanvasMode() && (leftDrawer.classList.contains("is-drawer-open") || rightDrawer.classList.contains("is-drawer-open"))) {
        setDrawer(null);
        return;
      }
      selectedNodeIds = new Set();
      selectedNodeId = null;
      if (latestDoc) drawGraph(latestDoc);
      return;
    }
    if (ev.key === "f" && document.activeElement !== canvasSearch) {
      ev.preventDefault();
      fitGraph();
      return;
    }
    const arrows = { ArrowLeft: [-16, 0], ArrowRight: [16, 0], ArrowUp: [0, -16], ArrowDown: [0, 16] };
    if (arrows[ev.key] && selectedNodeIds.size > 0) {
      ev.preventDefault();
      const step = ev.shiftKey ? 4 : 1;
      const [dx, dy] = arrows[ev.key];
      for (const id of selectedNodeIds) {
        const old = nodeOffsets.get(id) || { x: 0, y: 0 };
        nodeOffsets.set(id, { x: old.x + dx * step, y: old.y + dy * step });
      }
      if (latestDoc) drawGraph(latestDoc);
    }
  });

  window.addEventListener("keyup", function (ev) {
    if (ev.key === " ") spaceDown = false;
  });

  document.addEventListener("click", function (ev) {
    if (!contextMenu.contains(ev.target)) closeContextMenu();
  });

  function handleFunctionPinButton(ev) {
    const button = ev.target && ev.target.closest && ev.target.closest("#apply-function-pins, #set-function-output, #remove-function-output, #add-function-output");
    if (!button || !latestDoc) return;
    const now = Date.now();
    const handled_at = Number(button.getAttribute("data-canvas-handled-at") || "0");
    if (ev.type === "click" && now - handled_at < 900) return;
    button.setAttribute("data-canvas-handled-at", String(now));
    const graph = currentGraph(latestDoc);
    if (!graph || !graph.function) return;
    ev.preventDefault();
    ev.stopPropagation();
    if (ev.stopImmediatePropagation) ev.stopImmediatePropagation();
    if (button.id === "remove-function-output") {
      const ret = document.getElementById("function-return-type");
      if (ret) ret.value = "Void";
    } else if (button.id === "add-function-output") {
      const ret = document.getElementById("function-return-type");
      if (ret && (!ret.value.trim() || ret.value.trim() === "Void")) {
        ret.value = "Int";
        syncReturnEditorPreview(ret.value);
        window.__jetCanvasLastSignature = signatureFromVisibleFunctionPins(graph.function, graph.title);
        showToast("Output pin ready");
        return;
      }
    }
    applyFunctionPins(graph, graph.function, graph.title, button.id === "remove-function-output" ? "Void" : undefined);
  }
  function runPalette(item) {
    if (!latestDoc || !selectedGraphId) return;
    if (item.op === "preview_canvas_action") {
      postTransaction({ schema_version: 1, op: "preview_canvas_action", revision: latestDoc.revision, graph_id: selectedGraphId, action_id: item.action_id, callee: item.callee, args: item.args || ["\"canvas\""] });
    } else if (item.op === "insert_print") {
      postTransaction({ schema_version: 1, op: "insert_call", revision: latestDoc.revision, graph_id: selectedGraphId, callee: "print", args: ["\"canvas\""] });
    } else if (item.op === "insert_call") {
      const callee = window.prompt("Call function", "print");
      if (callee) postTransaction({ schema_version: 1, op: "insert_call", revision: latestDoc.revision, graph_id: selectedGraphId, callee, args: ["\"canvas\""] });
    } else if (["insert_branch", "insert_switch", "insert_loop", "insert_fallible_rail"].includes(item.op)) {
      postTransaction({ schema_version: 1, op: item.op, revision: latestDoc.revision, graph_id: selectedGraphId });
    } else if (item.op === "comment") {
      const graph = currentGraph(latestDoc);
      const node = graph && (graph.nodes.find((n) => n.node_id === selectedNodeId) || graph.nodes[0]);
      if (!node || !node.source_span) return showToast("Select a source node first");
      const b = { x: nodeX(node) - 28, y: nodeY(node) - 40, w: 246, h: 166 };
      const title = window.prompt("Comment title", "Comment");
      if (title) postTransaction({
        schema_version: 1,
        op: "create_comment_region",
        revision: latestDoc.revision,
        graph_id: selectedGraphId,
        start: node.source_span.start,
        end: node.source_span.end,
        title,
        color: "#2563eb",
        alpha: "0.18",
        bounds: [b.x, b.y, b.w, b.h].map((n) => Math.round(n)).join(",")
      });
    } else {
      showToast(item.title + " needs the next write transaction");
    }
  }

  function postTransaction(body) {
    const txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    const beforeSource = latestDoc && latestDoc.source_text;
    window.__jetCanvasLastTx = body;
    fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        window.__jetCanvasLastTxResult = result.json;
        if (!result.ok) { showToast(result.json.message || "Edit rejected"); return; }
        if (result.json.protocol === "jet.canvas.action") {
          searchState.results = [];
          searchState.spans = [];
          searchState.active = -1;
          searchState.impact = null;
          searchState.diff = { text: result.json.diff || "clean" };
          renderSearchResults();
          showToast("Canvas action preview validated");
          return;
        }
        if (result.json.changed && beforeSource && result.json.source_text && body.op !== "replace_source") {
          undoStack.push({ before: beforeSource, after: result.json.source_text });
          redoStack = [];
        }
        showToast(result.json.changed ? "Source updated" : "No change");
        loadSourceControl();
        loadGraph();
      })
      .catch((e) => showToast(String(e)));
  }

  function restoreSource(source, redoEntry, undoEntry) {
    if (!latestDoc || !source) return;
    const txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ schema_version: 1, op: "replace_source", revision: latestDoc.revision, source }) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) { showToast(result.json.message || "Undo rejected"); return; }
        if (redoEntry) redoStack.push(redoEntry);
        if (undoEntry) undoStack.push(undoEntry);
        showToast("Source restored");
        loadSourceControl();
        loadGraph();
      })
      .catch((e) => showToast(String(e)));
  }

  function undoTransaction() {
    const entry = undoStack.pop();
    if (!entry) return showToast("Nothing to undo");
    restoreSource(entry.before, entry);
  }

  function redoTransaction() {
    const entry = redoStack.pop();
    if (!entry) return showToast("Nothing to redo");
    restoreSource(entry.after, null, entry);
  }

  function loadGraph() {
    return fetch(graphUrl, { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        latestDoc = doc;
        sourceView.textContent = doc.source_text || "";
        const firstLoad = selectedGraphId === null;
        drawGraph(doc);
        setViewMode(viewMode);
        loadSourceControl();
        loadCanvasActions();
        applySourceHash();
        if (firstLoad) fitGraph();
      })
      .catch((e) => { jump.textContent = "Canvas graph failed"; details.textContent = String(e); });
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;" }[c]));
  }

  function escapeAttr(s) {
    return escapeHtml(s).replace(/`/g, "&#96;");
  }

  function loadCanvasActions() {
    if (!latestDoc) return;
    fetch(queryUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ schema_version: 1, revision: latestDoc.revision, op: "actions" })
    })
      .then((r) => r.json())
      .then((doc) => {
        if (!doc || !doc.actions) return;
        actionEntries = doc.actions.map((action) => ({
          title: action.title || action.callee,
          detail: (action.kind || "canvas.action") + " · " + (action.engine || "checked-tir+jit") + " · " + (action.callee || "") + "(" + (action.pins || []).filter((p) => p.direction === "input").map((p) => p.type || "Value").join(", ") + ") -> " + (action.ret || "Void"),
          op: "preview_canvas_action",
          action_id: action.action_id,
          callee: action.callee,
          pins: action.pins || [],
          args: action.default_args || ["\"canvas\""]
        }));
      })
      .catch(() => {});
  }

  function cssEscape(s) {
    if (window.CSS && CSS.escape) return CSS.escape(s);
    return String(s).replace(/["\\]/g, "\\$&");
  }

  window.__jetCanvasPinAuthoring = true;
  window.__jetCanvasDebugOverlay = true;
  setDeveloperMode(storedFlag("jet.canvas.developerMode"));
  details.innerHTML = "<h2>Details</h2><p>Select a node.</p>";
  window.addEventListener("resize", function () {
    if (!compactCanvasMode()) setDrawer(null);
    if (latestDoc) window.requestAnimationFrame(fitGraph);
  });

  const base = window.__JET_CANVAS_BASE__ || "/canvas";
  const graphUrl = window.__JET_CANVAS_GRAPH__ || (base + "/graph");
  const queryUrl = window.__JET_CANVAS_QUERY__ || (base + "/query");
  const sourceControlUrl = window.__JET_CANVAS_SCM__ || (base + "/source-control");
  loadGraph();
})();
"###
    .to_string()
}

fn project_file(path: &Path) -> Result<Projection, Vec<Diagnostic>> {
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

fn project_checked(
    path: &Path,
    src: &str,
    bundle: &AST::ProgramBundle,
    facts: &SemIndexEffectFacts,
) -> Projection {
    let index = jet_semindex::from_checked(bundle, facts);
    let mut graph_json = Vec::new();
    let mut inline_spans = Vec::new();
    let mut anchors = Vec::new();
    let mut node_refs = Vec::new();
    for module in &bundle.modules {
        collect_item_graphs(
            path,
            src,
            &index,
            &module.display,
            &module.source,
            &module.items,
            &mut graph_json,
            &mut inline_spans,
            &mut anchors,
            &mut node_refs,
        );
    }
    let fmt = crate::format_source(src).unwrap_or_else(|_| src.to_string());
    let json = format!(
        "{{\"protocol\":\"jet.canvas.graph\",\"schema_version\":{},\"source_id\":{},\"revision\":{},\"fmt_fingerprint\":{},\"source_text\":{},\"graphs\":[{}],\"diagnostics\":[],\"facts\":{{\"semindex_schema_version\":{},\"handles\":[\"definitions\",\"references\",\"calls\",\"effects\",\"members\"]}}}}",
        GRAPH_SCHEMA_VERSION,
        json_str(&path.display().to_string()),
        json_str(&source_revision(src)),
        json_str(&source_revision(&fmt)),
        json_str(src),
        graph_json.join(","),
        index.schema_version()
    );
    Projection {
        json,
        inline_exprs: inline_spans,
        graph_anchors: anchors,
        node_refs,
    }
}

fn collect_item_graphs(
    entry_path: &Path,
    entry_src: &str,
    index: &SemIndex,
    module_display: &str,
    module_src: &str,
    items: &[Item],
    out: &mut Vec<String>,
    inline_spans: &mut Vec<InlineExpr>,
    anchors: &mut Vec<GraphEditAnchor>,
    node_refs: &mut Vec<NodeQueryRef>,
) {
    for item in items {
        match item {
            Item::Func(f) => {
                let graph = project_func(index, module_display, module_src, f);
                inline_spans.extend(graph.inline_exprs.iter().map(|i| InlineExpr {
                    id: i.id.clone(),
                    span: i.span,
                }));
                anchors.push(GraphEditAnchor {
                    graph_id: graph.graph_id.clone(),
                    insert_offset: insert_offset(entry_src, f),
                });
                collect_node_refs(&graph, node_refs);
                out.push(graph_to_json(&graph, f, module_src));
            }
            Item::Struct(s) => {
                for method in &s.methods {
                    let graph = project_func(index, module_display, module_src, method);
                    inline_spans.extend(graph.inline_exprs.iter().map(|i| InlineExpr {
                        id: i.id.clone(),
                        span: i.span,
                    }));
                    anchors.push(GraphEditAnchor {
                        graph_id: graph.graph_id.clone(),
                        insert_offset: insert_offset(entry_src, method),
                    });
                    collect_node_refs(&graph, node_refs);
                    out.push(graph_to_json(&graph, method, module_src));
                }
            }
            Item::Impl(i) => {
                for method in &i.methods {
                    let graph = project_func(index, module_display, module_src, method);
                    inline_spans.extend(graph.inline_exprs.iter().map(|e| InlineExpr {
                        id: e.id.clone(),
                        span: e.span,
                    }));
                    anchors.push(GraphEditAnchor {
                        graph_id: graph.graph_id.clone(),
                        insert_offset: insert_offset(entry_src, method),
                    });
                    collect_node_refs(&graph, node_refs);
                    out.push(graph_to_json(&graph, method, module_src));
                }
            }
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    collect_item_graphs(
                        entry_path,
                        entry_src,
                        index,
                        module_display,
                        module_src,
                        body,
                        out,
                        inline_spans,
                        anchors,
                        node_refs,
                    );
                }
            }
            _ => {}
        }
    }
    let _ = entry_path;
}

fn project_func(
    index: &SemIndex,
    module_display: &str,
    module_src: &str,
    f: &AST::Func,
) -> GraphBuilder {
    let graph_id = graph_id(module_display, f);
    let mut g = GraphBuilder {
        graph_id: graph_id.clone(),
        ..GraphBuilder::default()
    };
    let entry_id = format!("{graph_id}:entry");
    g.nodes.push(NodeRec {
        id: entry_id.clone(),
        kind: "entry".to_string(),
        title: f.name.clone(),
        span: f.name_span.into(),
        x: 40,
        y: 40,
        badges: effect_badges(index, &f.name)
            .into_iter()
            .map(str::to_string)
            .collect(),
        affordances: vec![
            "source_jump".to_string(),
            "insert_call".to_string(),
            "rename_function".to_string(),
            "edit_function_signature".to_string(),
            "create_function".to_string(),
        ],
    });
    for (i, p) in f.params.iter().enumerate() {
        let pin_id = format!("{entry_id}:out:{}", p.name);
        let ty = p.ty.name();
        g.local_pins.insert(p.name.clone(), pin_id.clone());
        g.local_types.insert(p.name.clone(), ty.clone());
        g.pins.push(PinRec {
            id: pin_id,
            node_id: entry_id.clone(),
            name: p.name.clone(),
            direction: "output".to_string(),
            ty,
            capability: p.convention.sigil().to_string(),
            fallible: false,
            effect_grant_need: None,
            span: p.name_span.into(),
        });
        let _ = i;
    }
    for def in index.definitions() {
        match &def.kind {
            SymbolKind::Local { ty, .. } => {
                if let Some(t) = ty {
                    g.local_types.insert(def.name.clone(), t.clone());
                }
            }
            SymbolKind::Param { ty } => {
                g.local_types.insert(def.name.clone(), ty.clone());
            }
            _ => {}
        }
    }
    project_stmt_block(&mut g, index, module_src, &f.body, 0, 220, 170);
    add_source_comment_regions(&mut g, module_src, f);
    add_execution_overlay(&mut g);
    g
}

fn collect_node_refs(graph: &GraphBuilder, out: &mut Vec<NodeQueryRef>) {
    for node in &graph.nodes {
        out.push(NodeQueryRef {
            graph_id: graph.graph_id.clone(),
            node_id: node.id.clone(),
            kind: node.kind.clone(),
            title: node.title.clone(),
            span: node.span,
        });
    }
    for inline in &graph.inline_exprs {
        out.push(NodeQueryRef {
            graph_id: graph.graph_id.clone(),
            node_id: inline.node_id.clone(),
            kind: format!("inline:{}", inline.role),
            title: inline.source.clone(),
            span: inline.span,
        });
    }
}

fn project_stmt_block(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    stmts: &[Stmt],
    base: usize,
    x: i32,
    y: i32,
) {
    for (i, stmt) in stmts.iter().enumerate() {
        project_stmt(g, index, src, stmt, base + i + 1, x, y + i as i32 * 130);
    }
}

fn project_stmt(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    stmt: &Stmt,
    ordinal: usize,
    x: i32,
    y: i32,
) {
    match stmt {
        Stmt::Val(b) => {
            let node_id = format!("{}:stmt:{ordinal}:binding", g.graph_id);
            let ty = binding_type(g, &b.name, b);
            add_node(
                g,
                &node_id,
                "binding",
                &b.name,
                b.name_span.into(),
                x,
                y,
                vec!["local"],
                vec!["rename_binding", "edit_inline_expr", "source_jump"],
            );
            let input_pin = add_pin(g, &node_id, "value", "input", &ty, "", false);
            let output_pin = add_pin(g, &node_id, &b.name, "output", &ty, "", false);
            g.local_pins.insert(b.name.clone(), output_pin);
            g.local_types.insert(b.name.clone(), ty);
            connect_expr_to_input(
                g,
                index,
                src,
                &b.init,
                ordinal,
                "init",
                &node_id,
                &input_pin,
                x - 220,
                y,
            );
        }
        Stmt::Assign {
            target, op, value, ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:assign", g.graph_id);
            let title = assignment_title(target, *op);
            let ty = lvalue_type(g, target);
            add_node(
                g,
                &node_id,
                "assign",
                &title,
                target.span().into(),
                x,
                y,
                vec!["write"],
                vec!["edit_inline_expr", "source_jump"],
            );
            let input = add_pin(g, &node_id, "value", "input", &ty, "", false);
            let output = add_pin(g, &node_id, "target", "output", &ty, "&", false);
            connect_expr_to_input(
                g,
                index,
                src,
                value,
                ordinal,
                "value",
                &node_id,
                &input,
                x - 220,
                y,
            );
            if let AST::LValue::Local { name, .. } = target {
                g.local_pins.insert(name.clone(), output);
                g.local_types.insert(name.clone(), ty);
            }
        }
        Stmt::Expr(e) => {
            let _ = project_expr_node(g, index, src, e, ordinal, x, y);
        }
        Stmt::Return(expr, span) => {
            let node_id = format!("{}:stmt:{ordinal}:return", g.graph_id);
            add_node(
                g,
                &node_id,
                "return",
                "return",
                (*span).into(),
                x,
                y,
                vec!["exit"],
                vec!["source_jump"],
            );
            if let Some(e) = expr {
                let ty = expr_type(g, index, e);
                let input = add_pin(g, &node_id, "value", "input", &ty, "", false);
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    e,
                    ordinal,
                    "value",
                    &node_id,
                    &input,
                    x - 220,
                    y,
                );
            }
        }
        Stmt::If(ifs) => {
            let node_id = format!("{}:stmt:{ordinal}:branch", g.graph_id);
            add_node(
                g,
                &node_id,
                "branch",
                "if",
                ifs.cond.span().into(),
                x,
                y,
                vec!["control"],
                vec!["edit_inline_expr", "source_jump"],
            );
            let cond = add_pin(g, &node_id, "cond", "input", "Bool", "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                &ifs.cond,
                ordinal,
                "cond",
                &node_id,
                &cond,
                x - 220,
                y,
            );
            project_stmt_block(
                g,
                index,
                src,
                &ifs.then_body,
                ordinal * 100 + 10,
                x + 230,
                y + 70,
            );
            project_else_branch(
                g,
                index,
                src,
                ifs.else_branch.as_ref(),
                ordinal,
                x + 460,
                y + 70,
            );
        }
        Stmt::While {
            cond, body, span, ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:loop", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                "loop",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["edit_inline_expr", "source_jump"],
            );
            add_inline(g, &node_id, ordinal, "cond", src, cond.span());
            project_stmt_block(g, index, src, body, ordinal * 100 + 15, x + 230, y + 70);
        }
        Stmt::Loop { body, span, .. } => {
            let node_id = format!("{}:stmt:{ordinal}:loop", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                "loop",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["source_jump"],
            );
            project_stmt_block(g, index, src, body, ordinal * 100 + 20, x + 230, y + 70);
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            span,
            ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:counted_loop", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                "counted loop",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["edit_inline_expr", "source_jump"],
            );
            add_inline(g, &node_id, ordinal, "init", src, init.name_span);
            add_inline(g, &node_id, ordinal, "cond", src, cond.span());
            project_stmt(g, index, src, step, ordinal * 100 + 21, x + 230, y + 70);
            project_stmt_block(g, index, src, body, ordinal * 100 + 30, x + 230, y + 200);
        }
        Stmt::For {
            var,
            kind,
            body,
            span,
            ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:for", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                &format!("loop {var}"),
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["source_jump"],
            );
            let iter_ty = match kind {
                AST::ForKind::Range { .. } => "Int",
                AST::ForKind::In { .. } => "iterable",
            };
            let output = add_pin(g, &node_id, var, "output", iter_ty, "", false);
            g.local_pins.insert(var.clone(), output);
            g.local_types.insert(var.clone(), iter_ty.to_string());
            project_stmt_block(g, index, src, body, ordinal * 100 + 40, x + 230, y + 70);
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            span,
        } => {
            let node_id = format!("{}:stmt:{ordinal}:dispatch", g.graph_id);
            add_node(
                g,
                &node_id,
                "dispatch",
                "if ==",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["edit_inline_expr", "source_jump"],
            );
            let ty = expr_type(g, index, subject);
            let subject_pin = add_pin(g, &node_id, "subject", "input", &ty, "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                subject,
                ordinal,
                "subject",
                &node_id,
                &subject_pin,
                x - 220,
                y,
            );
            for (i, arm) in arms.iter().enumerate() {
                add_inline(
                    g,
                    &node_id,
                    ordinal,
                    &format!("arm{}", i + 1),
                    src,
                    arm.cond.span(),
                );
                project_stmt_block(
                    g,
                    index,
                    src,
                    &arm.body,
                    ordinal * 100 + 50 + i * 20,
                    x + 230 + i as i32 * 180,
                    y + 100,
                );
            }
            if let Some(body) = else_body {
                project_stmt_block(g, index, src, body, ordinal * 100 + 90, x + 460, y + 230);
            }
        }
        Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::BreakLabel(_, span)
        | Stmt::ContinueLabel(_, span) => {
            let node_id = format!("{}:stmt:{ordinal}:flow", g.graph_id);
            let title = snippet(src, *span);
            add_node(
                g,
                &node_id,
                "flow",
                &title,
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["source_jump"],
            );
        }
        Stmt::Unsafe { span, audit, body } => {
            g.regions.push(format!(
                "{{\"region_id\":{},\"kind\":\"unsafe\",\"title\":{},\"source_span\":{}}}",
                json_str(&format!("{}:region:{ordinal}:unsafe", g.graph_id)),
                json_str(audit.as_deref().unwrap_or("#Unsafe")),
                span_json((*span).into())
            ));
            project_stmt_block(g, index, src, body, ordinal * 100 + 95, x + 230, y + 70);
        }
        Stmt::Impure { reason, body, span } => {
            add_region(
                g,
                ordinal,
                "impure",
                reason.as_deref().unwrap_or("#Impure"),
                *span,
            );
            project_stmt_block(g, index, src, body, ordinal * 100 + 100, x + 230, y + 70);
        }
        Stmt::Reactive { body, span } => {
            add_region(g, ordinal, "reactive", "#Reactive", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 110, x + 230, y + 70);
        }
        Stmt::SuppressMustUse { body, span } => {
            add_region(g, ordinal, "suppress", "#Suppress(MustUse)", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 120, x + 230, y + 70);
        }
        Stmt::Region {
            name, body, span, ..
        } => {
            add_region(g, ordinal, "region", name, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 130, x + 230, y + 70);
        }
        Stmt::TaskGroup {
            name, body, span, ..
        } => {
            add_region(g, ordinal, "taskgroup", name, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 140, x + 230, y + 70);
        }
        Stmt::Layout {
            name, body, span, ..
        } => {
            add_region(g, ordinal, "layout", name, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 150, x + 230, y + 70);
        }
        Stmt::Caps {
            caps, body, span, ..
        } => {
            let title = caps
                .iter()
                .map(|(cap, _)| cap.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            add_region(g, ordinal, "caps", &title, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 160, x + 230, y + 70);
        }
        Stmt::Grant {
            caps,
            binding,
            body,
            span,
            ..
        } => {
            let title = format!(
                "{} -> {binding}",
                caps.iter()
                    .map(|(cap, _)| cap.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            add_region(g, ordinal, "grant", &title, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 165, x + 230, y + 70);
        }
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            span,
            ..
        } => {
            add_region(g, ordinal, "comptime_if", "comptime if", *span);
            let node_id = format!("{}:stmt:{ordinal}:comptime_if", g.graph_id);
            add_node(
                g,
                &node_id,
                "branch",
                "comptime if",
                (*span).into(),
                x,
                y,
                vec!["comptime"],
                vec!["edit_inline_expr", "source_jump"],
            );
            add_inline(g, &node_id, ordinal, "cond", src, cond.span());
            project_stmt_block(
                g,
                index,
                src,
                then_body,
                ordinal * 100 + 170,
                x + 230,
                y + 70,
            );
            if let Some(body) = else_body {
                project_stmt_block(g, index, src, body, ordinal * 100 + 180, x + 460, y + 70);
            }
        }
        Stmt::ComptimeBlock { body, span } => {
            add_region(g, ordinal, "comptime", "comptime", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 190, x + 230, y + 70);
        }
        Stmt::ContextBlock { body, span, .. }
        | Stmt::Live { body, span }
        | Stmt::AssumeDet { body, span }
        | Stmt::Transact { body, span, .. } => {
            add_region(g, ordinal, "scope", "scope", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 200, x + 230, y + 70);
        }
        Stmt::Yield(expr, span) => {
            let node_id = format!("{}:stmt:{ordinal}:yield", g.graph_id);
            add_node(
                g,
                &node_id,
                "yield",
                "yield",
                (*span).into(),
                x,
                y,
                vec!["stream"],
                vec!["source_jump"],
            );
            let ty = expr_type(g, index, expr);
            let input = add_pin(g, &node_id, "value", "input", &ty, "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                expr,
                ordinal,
                "value",
                &node_id,
                &input,
                x - 220,
                y,
            );
        }
        Stmt::ScopeMember {
            name,
            args,
            body,
            span,
            ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:scope_member:{name}", g.graph_id);
            add_node(
                g,
                &node_id,
                "scope_member",
                &format!(".{name}"),
                (*span).into(),
                x,
                y,
                vec!["scope"],
                vec!["source_jump"],
            );
            for (i, arg) in args.iter().enumerate() {
                add_inline(
                    g,
                    &node_id,
                    ordinal,
                    &format!("arg{}", i + 1),
                    src,
                    arg.span(),
                );
            }
            project_stmt_block(g, index, src, body, ordinal * 100 + 210, x + 230, y + 70);
        }
    }
}

fn project_else_branch(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    branch: Option<&AST::ElseBranch>,
    ordinal: usize,
    x: i32,
    y: i32,
) {
    match branch {
        Some(AST::ElseBranch::ElseIf(ifs)) => {
            project_stmt(
                g,
                index,
                src,
                &Stmt::If((**ifs).clone()),
                ordinal * 100 + 60,
                x,
                y,
            );
        }
        Some(AST::ElseBranch::Else(body)) => {
            project_stmt_block(g, index, src, body, ordinal * 100 + 70, x, y);
        }
        None => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn connect_expr_to_input(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    ordinal: usize,
    role: &str,
    owner_node_id: &str,
    input_pin: &str,
    x: i32,
    y: i32,
) {
    if let Some(out) = project_value_node(g, index, src, expr, ordinal, role, x, y) {
        add_inline(g, owner_node_id, ordinal, role, src, expr.span());
        add_wire_with_span(g, &out, input_pin, "data", Some(expr.span().into()));
    } else if pure_leaf(expr) {
        add_inline(g, owner_node_id, ordinal, role, src, expr.span());
        wire_ident_refs(g, expr, input_pin);
    } else if let Some(out) = project_expr_node(g, index, src, expr, ordinal, x, y) {
        add_wire_with_span(g, &out, input_pin, "data", Some(expr.span().into()));
    }
}

fn project_value_node(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    ordinal: usize,
    role: &str,
    x: i32,
    y: i32,
) -> Option<String> {
    let (kind, title, badges) = match expr {
        Expr::Ident(name, _) => ("variable_get", format!("get {name}"), vec!["read"]),
        Expr::Int(_, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Str(_, _)
        | Expr::Char(_, _)
        | Expr::Absent(_)
        | Expr::Todo { .. } => ("constant", snippet(src, expr.span()), vec!["const"]),
        _ => return None,
    };
    let span: SourceSpan = expr.span().into();
    let node_id = format!(
        "{}:value:{ordinal}:{role}:{}-{}",
        g.graph_id, span.start, span.end
    );
    add_node(
        g,
        &node_id,
        kind,
        &title,
        span,
        x,
        y,
        badges,
        vec!["edit_inline_expr", "source_jump"],
    );
    Some(add_pin(
        g,
        &node_id,
        "value",
        "output",
        &expr_type(g, index, expr),
        "",
        false,
    ))
}

fn project_expr_node(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    ordinal: usize,
    x: i32,
    y: i32,
) -> Option<String> {
    match expr {
        Expr::Call(c) => {
            let node_id = format!("{}:expr:{ordinal}:call:{}", g.graph_id, c.name);
            add_node(
                g,
                &node_id,
                "call",
                &c.name,
                c.name_span.into(),
                x,
                y,
                effect_badges(index, &c.name),
                vec!["insert_call", "source_jump"],
            );
            for (i, arg) in c.args.iter().enumerate() {
                let ty = expr_type(g, index, &arg.expr);
                let input = add_pin(
                    g,
                    &node_id,
                    &format!("arg{}", i + 1),
                    "input",
                    &ty,
                    arg.convention.sigil(),
                    false,
                );
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    &arg.expr,
                    ordinal * 1000 + i + 1,
                    &format!("arg{}", i + 1),
                    &node_id,
                    &input,
                    x - 220,
                    y + i as i32 * 74,
                );
            }
            let ret = call_ret(index, &c.name).unwrap_or_else(|| "Void".to_string());
            Some(add_pin(
                g,
                &node_id,
                "result",
                "output",
                &ret,
                "",
                ret.ends_with('?'),
            ))
        }
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            resolved_ret,
            ..
        } => {
            let node_id = format!("{}:expr:{ordinal}:method:{method}", g.graph_id);
            let variant_like = starts_uppercase(method);
            let kind = if variant_like { "variant" } else { "method" };
            let title = if variant_like {
                method.clone()
            } else {
                format!(".{method}")
            };
            add_node(
                g,
                &node_id,
                kind,
                &title,
                (*method_span).into(),
                x,
                y,
                Vec::new(),
                vec!["insert_call", "source_jump"],
            );
            let recv_ty = expr_type(g, index, receiver);
            let recv_pin = add_pin(g, &node_id, "self", "input", &recv_ty, "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                receiver,
                ordinal * 1000 + 1,
                "self",
                &node_id,
                &recv_pin,
                x - 220,
                y,
            );
            for (i, arg) in args.iter().enumerate() {
                let ty = expr_type(g, index, &arg.expr);
                let input = add_pin(
                    g,
                    &node_id,
                    &format!("arg{}", i + 1),
                    "input",
                    &ty,
                    arg.convention.sigil(),
                    false,
                );
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    &arg.expr,
                    ordinal * 1000 + i + 2,
                    &format!("arg{}", i + 1),
                    &node_id,
                    &input,
                    x - 220,
                    y + (i as i32 + 1) * 74,
                );
            }
            let ret = resolved_ret
                .as_ref()
                .map(AST::Type::name)
                .unwrap_or_else(|| "unknown".to_string());
            Some(add_pin(
                g,
                &node_id,
                "result",
                "output",
                &ret,
                "",
                ret.ends_with('?'),
            ))
        }
        Expr::Try(inner, span, _) => {
            let node_id = format!("{}:expr:{ordinal}:fallible", g.graph_id);
            add_node(
                g,
                &node_id,
                "fallible",
                "?",
                (*span).into(),
                x,
                y,
                vec!["fallible"],
                vec!["add_fallback_rail", "source_jump"],
            );
            let input = add_pin(
                g,
                &node_id,
                "value",
                "input",
                &expr_type(g, index, inner),
                "",
                true,
            );
            if let Some(out) = project_expr_node(g, index, src, inner, ordinal, x - 180, y) {
                add_wire(g, &out, &input, "fallible");
            }
            Some(add_pin(g, &node_id, "ok", "output", "unknown", "", false))
        }
        _ => {
            let node_id = format!("{}:expr:{ordinal}:expr", g.graph_id);
            let title = expr_title(expr);
            add_node(
                g,
                &node_id,
                "expr",
                title,
                expr.span().into(),
                x,
                y,
                vec!["expression"],
                vec!["edit_inline_expr", "source_jump"],
            );
            add_inline(g, &node_id, ordinal, "value", src, expr.span());
            Some(add_pin(
                g,
                &node_id,
                "value",
                "output",
                &expr_type(g, index, expr),
                "",
                false,
            ))
        }
    }
}

fn add_node(
    g: &mut GraphBuilder,
    id: &str,
    kind: &str,
    title: &str,
    span: SourceSpan,
    x: i32,
    y: i32,
    badges: Vec<&str>,
    affordances: Vec<&str>,
) {
    g.nodes.push(NodeRec {
        id: id.to_string(),
        kind: kind.to_string(),
        title: title.to_string(),
        span,
        x,
        y,
        badges: badges.into_iter().map(str::to_string).collect(),
        affordances: affordances.into_iter().map(str::to_string).collect(),
    });
}

fn add_pin(
    g: &mut GraphBuilder,
    node_id: &str,
    name: &str,
    direction: &str,
    ty: &str,
    capability: &str,
    fallible: bool,
) -> String {
    let id = format!("{node_id}:{direction}:{name}");
    let span = g
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.span)
        .unwrap_or(SourceSpan { start: 0, end: 0 });
    g.pins.push(PinRec {
        id: id.clone(),
        node_id: node_id.to_string(),
        name: name.to_string(),
        direction: direction.to_string(),
        ty: ty.to_string(),
        capability: capability.to_string(),
        fallible,
        effect_grant_need: None,
        span,
    });
    id
}

fn add_wire(g: &mut GraphBuilder, from_pin: &str, to_pin: &str, kind: &str) {
    add_wire_with_span(g, from_pin, to_pin, kind, None);
}

fn add_wire_with_span(
    g: &mut GraphBuilder,
    from_pin: &str,
    to_pin: &str,
    kind: &str,
    span: Option<SourceSpan>,
) {
    g.next_wire += 1;
    g.wires.push(WireRec {
        id: format!("{}:wire:{}", g.graph_id, g.next_wire),
        from_pin: from_pin.to_string(),
        to_pin: to_pin.to_string(),
        kind: kind.to_string(),
        span,
    });
}

fn add_region(g: &mut GraphBuilder, ordinal: usize, kind: &str, title: &str, span: Span) {
    g.regions.push(format!(
        "{{\"region_id\":{},\"kind\":{},\"title\":{},\"source_span\":{}}}",
        json_str(&format!("{}:region:{ordinal}:{kind}", g.graph_id)),
        json_str(kind),
        json_str(title),
        span_json(span.into())
    ));
}

fn add_source_comment_regions(g: &mut GraphBuilder, src: &str, f: &AST::Func) {
    let func_span = func_source_span(f);
    for hint in canvas_comment_hints(src) {
        if !span_overlaps(hint.anchor, func_span) {
            continue;
        }
        g.regions.push(format!(
            "{{\"region_id\":{},\"kind\":\"comment\",\"title\":{},\"color\":{},\"alpha\":{},\"bounds\":{{\"x\":{},\"y\":{},\"w\":{},\"h\":{}}},\"source_span\":{},\"hint_span\":{}}}",
            json_str(&comment_region_id(&g.graph_id, hint.anchor)),
            json_str(&hint.title),
            json_str(&hint.color),
            json_str(&hint.alpha),
            hint.bounds.0,
            hint.bounds.1,
            hint.bounds.2,
            hint.bounds.3,
            span_json(hint.anchor),
            span_json(hint.hint_span)
        ));
    }
    for hint in canvas_collapse_hints(src) {
        if !span_overlaps(hint.anchor, func_span) {
            continue;
        }
        g.regions.push(format!(
            "{{\"region_id\":{},\"kind\":\"collapse\",\"title\":{},\"source_span\":{},\"hint_span\":{}}}",
            json_str(&collapse_region_id(&g.graph_id, hint.anchor)),
            json_str(&hint.title),
            span_json(hint.anchor),
            span_json(hint.hint_span)
        ));
    }
}

fn add_execution_overlay(g: &mut GraphBuilder) {
    let exec_nodes = execution_nodes_in_order(g);

    for node in &exec_nodes {
        if node.kind != "entry" {
            ensure_exec_pin(g, &node.id, "exec", "input", node.span);
        }
        if node.kind == "branch" || node.kind == "dispatch" {
            ensure_exec_pin(g, &node.id, "then", "output", node.span);
            ensure_exec_pin(g, &node.id, "else", "output", node.span);
        } else if node.kind != "return" {
            ensure_exec_pin(g, &node.id, "then", "output", node.span);
        }
    }

    let mut previous_out: Option<String> = None;
    for node in &exec_nodes {
        let input = format!("{}:input:exec", node.id);
        if let Some(from) = previous_out.take() {
            if node.kind != "entry" {
                add_wire(g, &from, &input, "control");
            }
        }
        previous_out = if node.kind == "return" {
            None
        } else {
            Some(format!("{}:output:then", node.id))
        };
    }
}

fn execution_nodes_in_order(g: &GraphBuilder) -> Vec<NodeRec> {
    let mut nodes = g
        .nodes
        .iter()
        .filter(|node| node_wants_exec(node))
        .cloned()
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| {
        (
            if node.kind == "entry" { 0 } else { 1 },
            node.span.start,
            node.y,
            node.x,
        )
    });

    let source_rank = nodes
        .iter()
        .enumerate()
        .map(|(rank, node)| (node.id.clone(), rank))
        .collect::<HashMap<_, _>>();
    let exec_ids = source_rank.keys().cloned().collect::<HashSet<_>>();
    let pin_nodes = g
        .pins
        .iter()
        .map(|pin| (pin.id.clone(), pin.node_id.clone()))
        .collect::<HashMap<_, _>>();
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    let mut indegree = nodes
        .iter()
        .map(|node| (node.id.clone(), 0usize))
        .collect::<HashMap<_, _>>();

    for wire in g.wires.iter().filter(|wire| wire.kind == "data") {
        let Some(from_node) = pin_nodes.get(&wire.from_pin) else {
            continue;
        };
        let Some(to_node) = pin_nodes.get(&wire.to_pin) else {
            continue;
        };
        if from_node == to_node || !exec_ids.contains(from_node) || !exec_ids.contains(to_node) {
            continue;
        }
        let slot = edges.entry(from_node.clone()).or_default();
        if !slot.contains(to_node) {
            slot.push(to_node.clone());
            *indegree.entry(to_node.clone()).or_default() += 1;
        }
    }

    let by_id = nodes
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let mut ready = nodes
        .iter()
        .filter(|node| indegree.get(&node.id).copied().unwrap_or(0) == 0)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    ready.sort_by_key(|id| source_rank.get(id).copied().unwrap_or(usize::MAX));

    let mut ordered = Vec::new();
    let mut emitted = HashSet::new();
    while let Some(id) = ready.first().cloned() {
        ready.remove(0);
        if !emitted.insert(id.clone()) {
            continue;
        }
        if let Some(node) = by_id.get(&id) {
            ordered.push(node.clone());
        }
        for next in edges.get(&id).cloned().unwrap_or_default() {
            let Some(degree) = indegree.get_mut(&next) else {
                continue;
            };
            *degree = degree.saturating_sub(1);
            if *degree == 0 {
                ready.push(next);
                ready.sort_by_key(|id| source_rank.get(id).copied().unwrap_or(usize::MAX));
            }
        }
    }

    if ordered.len() != nodes.len() {
        for node in nodes {
            if !emitted.contains(&node.id) {
                ordered.push(node);
            }
        }
    }
    ordered
}

fn node_wants_exec(node: &NodeRec) -> bool {
    matches!(
        node.kind.as_str(),
        "entry"
            | "binding"
            | "assign"
            | "return"
            | "branch"
            | "loop"
            | "dispatch"
            | "flow"
            | "yield"
            | "scope_member"
            | "call"
            | "fallible"
    )
}

fn ensure_exec_pin(
    g: &mut GraphBuilder,
    node_id: &str,
    name: &str,
    direction: &str,
    span: SourceSpan,
) -> String {
    let id = format!("{node_id}:{direction}:{name}");
    if !g.pins.iter().any(|pin| pin.id == id) {
        g.pins.push(PinRec {
            id: id.clone(),
            node_id: node_id.to_string(),
            name: name.to_string(),
            direction: direction.to_string(),
            ty: "exec".to_string(),
            capability: "control".to_string(),
            fallible: false,
            effect_grant_need: None,
            span,
        });
    }
    id
}

fn func_source_span(f: &AST::Func) -> SourceSpan {
    let mut start = f.name_span.start;
    let mut end = f.name_span.end;
    for stmt in &f.body {
        let span = stmt.span();
        start = start.min(span.start);
        end = end.max(span.end);
    }
    SourceSpan { start, end }
}

#[derive(Clone)]
struct CommentHint {
    anchor: SourceSpan,
    hint_span: SourceSpan,
    title: String,
    color: String,
    alpha: String,
    bounds: (i32, i32, i32, i32),
}

fn canvas_comment_hints(src: &str) -> Vec<CommentHint> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(body) = trimmed.strip_prefix("// canvas:comment") {
            let anchor = attr_span(body, "span").unwrap_or(SourceSpan { start: 0, end: 0 });
            out.push(CommentHint {
                anchor,
                hint_span: SourceSpan {
                    start: offset,
                    end: offset + line.trim_end_matches('\n').len(),
                },
                title: attr_string(body, "title").unwrap_or_else(|| "Comment".to_string()),
                color: attr_string(body, "color").unwrap_or_else(|| "#2563eb".to_string()),
                alpha: attr_string(body, "alpha").unwrap_or_else(|| "0.18".to_string()),
                bounds: attr_bounds(body, "bounds").unwrap_or((0, 0, 360, 180)),
            });
        }
        offset += line.len();
    }
    out
}

fn comment_region_id(graph_id: &str, span: SourceSpan) -> String {
    format!("{graph_id}:comment:{}-{}", span.start, span.end)
}

fn canvas_collapse_hints(src: &str) -> Vec<CommentHint> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(body) = trimmed.strip_prefix("// canvas:collapse") {
            let anchor = attr_span(body, "span").unwrap_or(SourceSpan { start: 0, end: 0 });
            out.push(CommentHint {
                anchor,
                hint_span: SourceSpan {
                    start: offset,
                    end: offset + line.trim_end_matches('\n').len(),
                },
                title: attr_string(body, "title").unwrap_or_else(|| "Collapsed".to_string()),
                color: "#64748b".to_string(),
                alpha: "0.16".to_string(),
                bounds: (0, 0, 360, 120),
            });
        }
        offset += line.len();
    }
    out
}

fn collapse_region_id(graph_id: &str, span: SourceSpan) -> String {
    format!("{graph_id}:collapse:{}-{}", span.start, span.end)
}

fn add_inline(
    g: &mut GraphBuilder,
    node_id: &str,
    ordinal: usize,
    role: &str,
    src: &str,
    span: Span,
) {
    let source = snippet(src, span);
    let id = format!("{}:inline:{ordinal}:{role}", g.graph_id);
    g.inline_exprs.push(InlineRec {
        id,
        node_id: node_id.to_string(),
        role: role.to_string(),
        source,
        span: span.into(),
    });
}

fn graph_to_json(g: &GraphBuilder, f: &AST::Func, src: &str) -> String {
    let nodes = g.nodes.iter().map(node_json).collect::<Vec<_>>().join(",");
    let pins = g.pins.iter().map(pin_json).collect::<Vec<_>>().join(",");
    let wires = g.wires.iter().map(wire_json).collect::<Vec<_>>().join(",");
    let inline = g
        .inline_exprs
        .iter()
        .map(inline_json)
        .collect::<Vec<_>>()
        .join(",");
    let exit_nodes = g
        .nodes
        .iter()
        .filter(|n| n.kind == "return")
        .map(|n| json_str(&n.id))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"graph_id\":{},\"title\":{},\"function\":{},\"event_views\":[{}],\"entry_node\":{},\"exit_nodes\":[{}],\"nodes\":[{}],\"pins\":[{}],\"wires\":[{}],\"regions\":[{}],\"inline_exprs\":[{}],\"rails\":{},\"layout_hints\":{{\"algorithm\":\"source-order-v1\",\"direction\":\"data-left-to-right/control-top-to-bottom\"}}}}",
        json_str(&g.graph_id),
        json_str(&f.name),
        function_metadata_json(src, f),
        callback_event_view_json(&g.graph_id, f).unwrap_or_default(),
        json_str(&format!("{}:entry", g.graph_id)),
        exit_nodes,
        nodes,
        pins,
        wires,
        g.regions.join(","),
        inline,
        rails_json(g)
    )
}

fn function_metadata_json(src: &str, f: &AST::Func) -> String {
    let params = f
        .params
        .iter()
        .map(|p| {
            let default = p
                .default
                .as_ref()
                .map(|expr| json_str(&snippet(src, expr.span())))
                .unwrap_or_else(|| "null".to_string());
            format!(
                "{{\"name\":{},\"type\":{},\"capability\":{},\"default\":{},\"default_source\":{},\"source_span\":{}}}",
                json_str(&p.name),
                json_str(&p.ty.name()),
                json_str(p.convention.sigil()),
                if p.default.is_some() { "true" } else { "false" },
                default,
                span_json(p.name_span.into())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let ret = f
        .return_type
        .as_ref()
        .map(AST::Type::name)
        .unwrap_or_else(|| "Void".to_string());
    let effects = f
        .declared_effects
        .as_ref()
        .map(|effects| {
            effects
                .iter()
                .map(|(name, _)| json_str(name))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    format!(
        "{{\"name\":{},\"signature\":{},\"visibility\":{},\"docs\":{},\"pure\":{},\"unsafe\":{},\"effects\":[{}],\"returns\":{},\"params\":[{}],\"source_span\":{},\"edit_affordances\":[\"rename_function\",\"edit_function_signature\",\"create_function\",\"source_jump\"]}}",
        json_str(&f.name),
        json_str(&function_signature_text(src, f)),
        json_str(function_visibility(f)),
        json_str(&doc_comment_before(src, f.name_span.start)),
        if f.is_pure { "true" } else { "false" },
        if f.is_unsafe { "true" } else { "false" },
        effects,
        json_str(&ret),
        params,
        span_json(func_source_span(f))
    )
}

fn function_visibility(f: &AST::Func) -> &'static str {
    if f.is_package_pub {
        "package"
    } else if f.is_pub {
        "public"
    } else {
        "private"
    }
}

fn function_signature_text(src: &str, f: &AST::Func) -> String {
    let mut out = String::new();
    if f.is_pure {
        out.push_str("@Pure ");
    }
    if f.is_package_pub {
        out.push_str("pub(package) ");
    } else if f.is_pub {
        out.push_str("pub ");
    }
    out.push_str("fn ");
    out.push_str(&f.name);
    out.push('(');
    out.push_str(
        &f.params
            .iter()
            .map(|p| {
                let mut s = format!("{}: {}", p.name, p.ty.name());
                if let Some(default) = &p.default {
                    s.push_str(" = ");
                    s.push_str(&snippet(src, default.span()));
                }
                s
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push(')');
    if let Some(ret) = &f.return_type {
        out.push_str(" -> ");
        out.push_str(&ret.name());
    }
    out
}

fn doc_comment_before(src: &str, name_start: usize) -> String {
    let mut docs = Vec::new();
    let mut cursor = line_start(src, name_start);
    while cursor > 0 {
        let prev_end = cursor.saturating_sub(1);
        let prev_start = line_start(src, prev_end);
        let line = src[prev_start..prev_end].trim();
        if let Some(doc) = line.strip_prefix("///") {
            docs.push(doc.trim().to_string());
            cursor = prev_start;
            continue;
        }
        if line.is_empty() {
            cursor = prev_start;
            continue;
        }
        break;
    }
    docs.reverse();
    docs.join("\n")
}

fn callback_event_view_json(graph_id: &str, f: &AST::Func) -> Option<String> {
    let event = callback_event_name(&f.name)?;
    Some(format!(
        "{{\"event_id\":{},\"kind\":\"callback_event\",\"title\":{},\"function\":{},\"entry_node\":{},\"semantics\":\"ordinary_jet_function\",\"dispatch\":\"framework_callback\",\"pending_first_class_events\":\"#286\",\"source_span\":{}}}",
        json_str(&format!("{graph_id}:event:{event}")),
        json_str(&event),
        json_str(&f.name),
        json_str(&format!("{graph_id}:entry")),
        span_json(func_source_span(f))
    ))
}

fn callback_event_name(name: &str) -> Option<String> {
    name.strip_prefix("on_")
        .filter(|rest| !rest.is_empty())
        .map(|rest| rest.to_string())
}

fn rails_json(g: &GraphBuilder) -> String {
    let mut kinds = Vec::new();
    push_rail(&mut kinds, "control");
    if !g.wires.is_empty() || !g.pins.is_empty() {
        push_rail(&mut kinds, "data");
    }
    if g.wires.iter().any(|w| w.kind == "fallible") || g.pins.iter().any(|p| p.fallible) {
        push_rail(&mut kinds, "fallible");
    }
    if g.regions
        .iter()
        .any(|r| r.contains("\"kind\":\"taskgroup\""))
        || g.nodes
            .iter()
            .any(|n| n.title == ".task" || n.title == "spawn" || n.title.ends_with(".spawn"))
    {
        push_rail(&mut kinds, "async");
    }
    if g.nodes
        .iter()
        .any(|n| n.badges.iter().any(|b| b == "effects"))
        || g.regions
            .iter()
            .any(|r| r.contains("\"kind\":\"caps\"") || r.contains("\"kind\":\"grant\""))
    {
        push_rail(&mut kinds, "effect");
    }
    if g.regions
        .iter()
        .any(|r| r.contains("\"kind\":\"unsafe\"") || r.contains("\"kind\":\"comptime\""))
    {
        push_rail(&mut kinds, "proof");
    }
    push_rail(&mut kinds, "debug");
    format!(
        "{{\"kinds\":[{}],\"debug_overlay\":\"idle\",\"source\":\"front-end facts\"}}",
        json_strs(&kinds)
    )
}

fn push_rail(kinds: &mut Vec<String>, kind: &str) {
    if !kinds.iter().any(|k| k == kind) {
        kinds.push(kind.to_string());
    }
}

fn node_json(n: &NodeRec) -> String {
    format!(
        "{{\"node_id\":{},\"kind\":{},\"title\":{},\"source_span\":{},\"layout\":{{\"x\":{},\"y\":{}}},\"badges\":[{}],\"edit_affordances\":[{}]}}",
        json_str(&n.id),
        json_str(&n.kind),
        json_str(&n.title),
        span_json(n.span),
        n.x,
        n.y,
        json_strs(&n.badges),
        json_strs(&n.affordances)
    )
}

fn pin_json(p: &PinRec) -> String {
    let grant = p
        .effect_grant_need
        .as_ref()
        .map(|s| json_str(s))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"pin_id\":{},\"node_id\":{},\"name\":{},\"direction\":{},\"type\":{},\"capability\":{},\"fallible\":{},\"effect_grant_need\":{},\"source_span\":{}}}",
        json_str(&p.id),
        json_str(&p.node_id),
        json_str(&p.name),
        json_str(&p.direction),
        json_str(&p.ty),
        json_str(&p.capability),
        if p.fallible { "true" } else { "false" },
        grant,
        span_json(p.span)
    )
}

fn wire_json(w: &WireRec) -> String {
    let source_span = w.span.map(span_json).unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"wire_id\":{},\"from_pin\":{},\"to_pin\":{},\"wire_kind\":{},\"source_span\":{}}}",
        json_str(&w.id),
        json_str(&w.from_pin),
        json_str(&w.to_pin),
        json_str(&w.kind),
        source_span
    )
}

fn inline_json(i: &InlineRec) -> String {
    format!(
        "{{\"inline_expr_id\":{},\"node_id\":{},\"role\":{},\"source\":{},\"source_span\":{},\"editable\":true}}",
        json_str(&i.id),
        json_str(&i.node_id),
        json_str(&i.role),
        json_str(&i.source),
        span_json(i.span)
    )
}

fn canvas_find(path: &Path, src: &str, query: &str) -> Result<String, String> {
    let (projection, index) = open_query_context(path, src)?;
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(query_ok("find", src, "[]", "\"impact\":null,\"diff\":null"));
    }
    let mut results = Vec::new();
    for def in index.definitions() {
        if contains_ci(&def.name, needle) || contains_ci(&def.identity, needle) {
            results.push(query_result_json(
                "definition",
                &def.name,
                &def.name,
                &def.module_path,
                def.def_span,
                node_for_span(&projection, def.def_span).as_ref(),
                src,
            ));
        }
    }
    for r in index.references() {
        if contains_ci(&r.name, needle) {
            results.push(query_result_json(
                "reference",
                &r.name,
                &r.name,
                &r.module_path,
                r.span,
                node_for_span(&projection, r.span).as_ref(),
                src,
            ));
        }
    }
    for node in &projection.node_refs {
        if contains_ci(&node.title, needle) || contains_ci(&node.kind, needle) {
            results.push(query_result_json(
                "graph",
                &node.title,
                &node.title,
                "",
                node.span,
                Some(node),
                src,
            ));
        }
    }
    for span in text_matches(src, needle) {
        results.push(query_result_json(
            "source",
            needle,
            needle,
            &path.display().to_string(),
            span,
            node_for_span(&projection, span).as_ref(),
            src,
        ));
    }
    dedupe_results(&mut results);
    Ok(query_ok(
        "find",
        src,
        &format!("[{}]", results.join(",")),
        "\"impact\":null,\"diff\":null",
    ))
}

fn canvas_references(path: &Path, src: &str, symbol: &str) -> Result<String, String> {
    let (projection, index) = open_query_context(path, src)?;
    let mut results = Vec::new();
    for def in index.definitions().iter().filter(|d| d.name == symbol) {
        results.push(query_result_json(
            "definition",
            &def.name,
            &def.name,
            &def.module_path,
            def.def_span,
            node_for_span(&projection, def.def_span).as_ref(),
            src,
        ));
    }
    for r in index.references_to(symbol) {
        results.push(query_result_json(
            "reference",
            &r.name,
            &r.name,
            &r.module_path,
            r.span,
            node_for_span(&projection, r.span).as_ref(),
            src,
        ));
    }
    let impact = jet_impact::ImpactReport::analyze(&index, symbol, 3).to_json();
    Ok(query_ok(
        "references",
        src,
        &format!("[{}]", results.join(",")),
        &format!("\"impact\":{},\"diff\":null", impact),
    ))
}

fn canvas_source_to_graph(path: &Path, src: &str, span: SourceSpan) -> Result<String, String> {
    let projection =
        project_file(path).map_err(|diags| query_diagnostics_error(path, src, &diags))?;
    let mut results = projection
        .node_refs
        .iter()
        .filter(|node| spans_overlap(node.span, span))
        .map(|node| {
            query_result_json(
                "source_to_graph",
                &node.title,
                &node.title,
                "",
                node.span,
                Some(node),
                src,
            )
        })
        .collect::<Vec<_>>();
    if results.is_empty() {
        if let Some(node) = nearest_node(&projection, span.start) {
            results.push(query_result_json(
                "source_to_graph",
                &node.title,
                &node.title,
                "",
                node.span,
                Some(&node),
                src,
            ));
        }
    }
    Ok(query_ok(
        "source_to_graph",
        src,
        &format!("[{}]", results.join(",")),
        "\"impact\":null,\"diff\":null",
    ))
}

fn canvas_preview_rename(path: &Path, src: &str, symbol: &str, to: &str) -> Result<String, String> {
    let (projection, index) = open_query_context(path, src)?;
    let mut edits = Vec::new();
    let mut results = Vec::new();
    for def in index.definitions().iter().filter(|d| d.name == symbol) {
        edits.push(edit(def.def_span, to));
        results.push(query_result_json(
            "definition",
            &def.name,
            &def.name,
            &def.module_path,
            def.def_span,
            node_for_span(&projection, def.def_span).as_ref(),
            src,
        ));
    }
    for r in index.references_to(symbol) {
        edits.push(edit(r.span, to));
        results.push(query_result_json(
            "reference",
            &r.name,
            &r.name,
            &r.module_path,
            r.span,
            node_for_span(&projection, r.span).as_ref(),
            src,
        ));
    }
    if edits.is_empty() {
        return Err(query_error(
            "not_found",
            "Canvas rename preview found no matching symbol",
        ));
    }
    let changed = FixEngine::apply_edits(src, &edits)
        .map_err(|_| query_error("overlap", "Canvas rename preview edits overlapped"))?;
    let diff = simple_diff(src, &changed);
    Ok(query_ok(
        "preview_rename",
        src,
        &format!("[{}]", results.join(",")),
        &format!(
            "\"impact\":null,\"diff\":{{\"changed\":{},\"text\":{},\"after_revision\":{}}}",
            if changed != src { "true" } else { "false" },
            json_str(&diff),
            json_str(&source_revision(&changed))
        ),
    ))
}

fn canvas_actions(path: &Path, src: &str) -> Result<String, String> {
    let (_projection, index) = open_query_context(path, src)?;
    let mut entries = Vec::new();
    for def in index.definitions() {
        let SymbolKind::Function { params, ret } = &def.kind else {
            continue;
        };
        if def.name == "run" {
            continue;
        }
        entries.push(canvas_action_json(def, params, ret.as_deref()));
    }
    entries.push(canvas_builtin_action_json(
        "print",
        "Print",
        &[("value", "Any")],
        "Void",
        &["\"canvas\""],
    ));
    entries.sort();
    Ok(query_ok(
        "actions",
        src,
        "[]",
        &format!(
            "\"impact\":null,\"diff\":null,\"actions_schema_version\":{},\"actions\":[{}]",
            ACTION_SCHEMA_VERSION,
            entries.join(",")
        ),
    ))
}

fn canvas_action_json(
    def: &jet_semindex::SymbolDef,
    params: &[(String, String)],
    ret: Option<&str>,
) -> String {
    let action_id = format!("canvas.action:{}:{}", def.module_path, def.name);
    let default_args = params
        .iter()
        .map(|(_, ty)| json_str(&default_arg_for_type(ty)))
        .collect::<Vec<_>>()
        .join(",");
    let pins = params
        .iter()
        .map(|(name, ty)| {
            format!(
                "{{\"name\":{},\"direction\":\"input\",\"type\":{}}}",
                json_str(name),
                json_str(ty)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"action_id\":{},\"kind\":\"canvas.action\",\"title\":{},\"callee\":{},\"module_path\":{},\"engine\":\"checked-tir+jit\",\"authority\":[\"canvas.source_edit:current_file\"],\"writes\":\"source_transaction_only\",\"audit\":[\"package_id\",\"version\",\"hash\",\"authority\",\"touched_files\",\"diff\",\"diagnostics\"],\"source_span\":{},\"ret\":{},\"pins\":[{}],\"default_args\":[{}]}}",
        json_str(&action_id),
        json_str(&def.name),
        json_str(&def.name),
        json_str(&def.module_path),
        span_json(def.def_span),
        json_str(ret.unwrap_or("Void")),
        pins,
        default_args
    )
}

fn canvas_builtin_action_json(
    callee: &str,
    title: &str,
    params: &[(&str, &str)],
    ret: &str,
    default_args: &[&str],
) -> String {
    let action_id = format!("canvas.action:builtin:{callee}");
    let pins = params
        .iter()
        .map(|(name, ty)| {
            format!(
                "{{\"name\":{},\"direction\":\"input\",\"type\":{}}}",
                json_str(name),
                json_str(ty)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let default_args = default_args
        .iter()
        .map(|arg| json_str(arg))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"action_id\":{},\"kind\":\"canvas.builtin\",\"title\":{},\"callee\":{},\"module_path\":\"builtin\",\"engine\":\"checked-tir+jit\",\"authority\":[\"canvas.source_edit:current_file\"],\"writes\":\"source_transaction_only\",\"audit\":[\"package_id\",\"version\",\"hash\",\"authority\",\"touched_files\",\"diff\",\"diagnostics\"],\"source_span\":null,\"ret\":{},\"pins\":[{}],\"default_args\":[{}]}}",
        json_str(&action_id),
        json_str(title),
        json_str(callee),
        json_str(ret),
        pins,
        default_args
    )
}

fn default_arg_for_type(ty: &str) -> String {
    match ty {
        "Bool" => "true".to_string(),
        "String" => "\"canvas\"".to_string(),
        "Float" | "F32" | "F64" => "1.0".to_string(),
        _ => "1".to_string(),
    }
}

fn apply_noop(path: &Path, src: &str) -> Result<String, String> {
    let formatted =
        crate::format_source(src).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let changed = formatted != src;
    if changed {
        fs::write(path, formatted).map_err(|e| edit_error("io", &e.to_string()))?;
    }
    Ok(edit_ok(changed, path))
}

fn apply_rename(path: &Path, src: &str, from: &str, to: &str) -> Result<String, String> {
    let idx = jet_semindex::open(path).map_err(|e| edit_error("check", &e.to_string()))?;
    let mut edits = Vec::new();
    for def in idx.definitions().iter().filter(|d| d.name == from) {
        edits.push(edit(def.def_span, to));
    }
    for r in idx.references().iter().filter(|r| r.name == from) {
        edits.push(edit(r.span, to));
    }
    if edits.is_empty() {
        return Err(edit_error(
            "not_found",
            "Canvas rename found no matching binding",
        ));
    }
    let changed = FixEngine::apply_edits(src, &edits)
        .map_err(|_| edit_error("overlap", "Canvas rename edits overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_create_function(
    path: &Path,
    src: &str,
    name: &str,
    params: &str,
    ret_type: &str,
) -> Result<String, String> {
    let body = if ret_type == "Void" {
        String::new()
    } else {
        format!("    return {}\n", default_arg_for_type(ret_type))
    };
    let ret = if ret_type == "Void" {
        String::new()
    } else {
        format!(" -> {ret_type}")
    };
    let function = format!("fn {name}({params}){ret} {{\n{body}}}\n\n");
    let changed = FixEngine::apply_edits(src, &[edit(SourceSpan { start: 0, end: 0 }, &function)])
        .map_err(|_| edit_error("overlap", "Canvas create function edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_edit_function_signature(
    path: &Path,
    src: &str,
    graph_id: &str,
    signature: &str,
) -> Result<String, String> {
    let Some(name_span) = graph_id_name_span(graph_id) else {
        return Err(edit_error(
            "bad_request",
            "Canvas graph id is not a function graph",
        ));
    };
    let span = function_signature_span(src, name_span)?;
    let changed = FixEngine::apply_edits(src, &[edit(span, signature)])
        .map_err(|_| edit_error("overlap", "Canvas function signature edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_inline_edit(
    path: &Path,
    src: &str,
    inline_id: &str,
    new_expr: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(inline) = projection.inline_exprs.iter().find(|i| i.id == inline_id) else {
        return Err(edit_error(
            "not_found",
            "Canvas inline expression no longer exists",
        ));
    };
    let changed = FixEngine::apply_edits(src, &[edit(inline.span, new_expr)])
        .map_err(|_| edit_error("overlap", "Canvas inline edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_promote_inline(
    path: &Path,
    src: &str,
    inline_id: &str,
    name: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(inline) = projection.inline_exprs.iter().find(|i| i.id == inline_id) else {
        return Err(edit_error(
            "not_found",
            "Canvas inline expression no longer exists",
        ));
    };
    let expr = snippet(src, Span::new(inline.span.start, inline.span.end));
    let line = line_start(src, inline.span.start);
    let indent = indentation_at(src, inline.span.start);
    let insert = format!("{indent}{name} :: {expr}\n");
    let edits = [
        edit(
            SourceSpan {
                start: line,
                end: line,
            },
            &insert,
        ),
        edit(inline.span, name),
    ];
    let changed = FixEngine::apply_edits(src, &edits)
        .map_err(|_| edit_error("overlap", "Canvas promote edits overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_visible_conversion(
    path: &Path,
    src: &str,
    inline_id: &str,
    callee: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(inline) = projection.inline_exprs.iter().find(|i| i.id == inline_id) else {
        return Err(edit_error(
            "not_found",
            "Canvas inline expression no longer exists",
        ));
    };
    let expr = snippet(src, Span::new(inline.span.start, inline.span.end));
    let replacement = format!("{callee}({expr})");
    let changed = FixEngine::apply_edits(src, &[edit(inline.span, &replacement)])
        .map_err(|_| edit_error("overlap", "Canvas conversion edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_break_link(path: &Path, src: &str, wire_id: &str) -> Result<String, String> {
    rewrite_wire_expr(path, src, wire_id, "#Todo")
}

fn apply_move_link(
    path: &Path,
    src: &str,
    wire_id: &str,
    replacement: &str,
) -> Result<String, String> {
    rewrite_wire_expr(path, src, wire_id, replacement)
}

fn rewrite_wire_expr(
    path: &Path,
    src: &str,
    wire_id: &str,
    replacement: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(span) = projection
        .json
        .split("\"wire_id\":")
        .skip(1)
        .find_map(|chunk| wire_span_from_json_chunk(chunk, wire_id))
    else {
        return Err(edit_error(
            "not_found",
            "Canvas wire no longer maps to a source expression",
        ));
    };
    let changed = FixEngine::apply_edits(src, &[edit(span, replacement)])
        .map_err(|_| edit_error("overlap", "Canvas wire edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_insert_call(
    path: &Path,
    src: &str,
    graph_id: &str,
    callee: &str,
    args: &[String],
    bind: Option<&str>,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(anchor) = projection
        .graph_anchors
        .iter()
        .find(|a| a.graph_id == graph_id)
    else {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    };
    let call = format!("{callee}({})", args.join(", "));
    let stmt = match bind {
        Some(name) => format!("    {name} :: {call}\n"),
        None => format!("    {call}\n"),
    };
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: anchor.insert_offset,
                end: anchor.insert_offset,
            },
            &stmt,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas insert edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn canvas_action_candidate(
    path: &Path,
    src: &str,
    graph_id: &str,
    action_id: &str,
    callee: &str,
    args: &[String],
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(anchor) = projection
        .graph_anchors
        .iter()
        .find(|a| a.graph_id == graph_id)
    else {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    };
    if !action_id.starts_with("canvas.action:") {
        return Err(edit_error(
            "bad_request",
            "Canvas action id must be a package Canvas action",
        ));
    }
    let stmt = format!("    {callee}({})\n", args.join(", "));
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: anchor.insert_offset,
                end: anchor.insert_offset,
            },
            &stmt,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas action edit overlapped"))?;
    let formatted =
        crate::format_source(&changed).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let tmp = temp_canvas_check_path(path);
    fs::write(&tmp, &formatted).map_err(|e| edit_error("io", &e.to_string()))?;
    let (check, _) = crate::Driver::check_file(&tmp.display().to_string(), None, true);
    let _ = fs::remove_file(&tmp);
    let errors = check
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(diagnostics_error(path, &formatted, &errors));
    }
    Ok(formatted)
}

fn temp_canvas_check_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("canvas_action");
    tmp.set_file_name(format!("{stem}.canvas-action-check.jet"));
    tmp
}

fn apply_insert_structural(
    path: &Path,
    src: &str,
    graph_id: &str,
    op: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(anchor) = projection
        .graph_anchors
        .iter()
        .find(|a| a.graph_id == graph_id)
    else {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    };
    let stmt = match op {
        "insert_branch" => {
            "    if true {\n        print(\"branch\")\n    } else {\n        print(\"else\")\n    }\n".to_string()
        }
        "insert_switch" => {
            "    if 0 == {\n        0 -> { print(\"case\") }\n        else -> { print(\"else\") }\n    }\n"
                .to_string()
        }
        "insert_loop" => "    loop {\n        break\n    }\n".to_string(),
        "insert_fallible_rail" => {
            "    fallible_value: Int ? String :: ok(1)\n    unwrapped :: fallible_value?\n".to_string()
        }
        _ => return Err(edit_error("unsupported", "unknown Canvas structural operation")),
    };
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: anchor.insert_offset,
                end: anchor.insert_offset,
            },
            &stmt,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas structural insert overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_create_comment_region(
    path: &Path,
    src: &str,
    graph_id: &str,
    start: usize,
    end: usize,
    title: &str,
    color: &str,
    alpha: &str,
    bounds: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    if !projection
        .graph_anchors
        .iter()
        .any(|a| a.graph_id == graph_id)
    {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    }
    validate_comment_color(color)?;
    validate_comment_alpha(alpha)?;
    let bounds = normalize_bounds(bounds)?;
    let anchor = SourceSpan { start, end };
    let insert_at = line_after(src, end.min(src.len()));
    let indent = indentation_at(src, insert_at.min(src.len()));
    let comment = format!(
        "{indent}// canvas:comment span={}..{} title={} color={} alpha={} bounds=({})\n",
        anchor.start,
        anchor.end,
        quoted_attr(title),
        quoted_attr(color),
        alpha,
        bounds
    );
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: insert_at,
                end: insert_at,
            },
            &comment,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas comment insert overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_update_comment_region(
    path: &Path,
    src: &str,
    region_id: &str,
    title: Option<&str>,
    color: Option<&str>,
    alpha: Option<&str>,
    bounds: Option<&str>,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(hint) = find_comment_hint(src, &projection.json, region_id) else {
        return Err(edit_error(
            "not_found",
            "Canvas comment region no longer exists",
        ));
    };
    let color = color.unwrap_or(&hint.color);
    let alpha = alpha.unwrap_or(&hint.alpha);
    validate_comment_color(color)?;
    validate_comment_alpha(alpha)?;
    let fallback_bounds = format!(
        "{},{},{},{}",
        hint.bounds.0, hint.bounds.1, hint.bounds.2, hint.bounds.3
    );
    let bounds = normalize_bounds(bounds.unwrap_or(&fallback_bounds))?;
    let title = title.unwrap_or(&hint.title);
    let replacement = format!(
        "// canvas:comment span={}..{} title={} color={} alpha={} bounds=({})",
        hint.anchor.start,
        hint.anchor.end,
        quoted_attr(title),
        quoted_attr(color),
        alpha,
        bounds
    );
    let changed = FixEngine::apply_edits(src, &[edit(hint.hint_span, &replacement)])
        .map_err(|_| edit_error("overlap", "Canvas comment edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_delete_comment_region(path: &Path, src: &str, region_id: &str) -> Result<String, String> {
    apply_delete_hint_region(path, src, region_id, "comment")
}

fn apply_delete_hint_region(
    path: &Path,
    src: &str,
    region_id: &str,
    kind: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(hint) = find_hint_region(src, &projection.json, region_id, kind) else {
        return Err(edit_error(
            "not_found",
            "Canvas source hint no longer exists",
        ));
    };
    let mut span = hint.hint_span;
    if span.end < src.len() && src.as_bytes().get(span.end) == Some(&b'\n') {
        span.end += 1;
    }
    let changed = FixEngine::apply_edits(src, &[edit(span, "")])
        .map_err(|_| edit_error("overlap", "Canvas source hint delete overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn apply_create_collapse_region(
    path: &Path,
    src: &str,
    graph_id: &str,
    start: usize,
    end: usize,
    title: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    if !projection
        .graph_anchors
        .iter()
        .any(|a| a.graph_id == graph_id)
    {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    }
    let insert_at = line_after(src, end.min(src.len()));
    let indent = indentation_at(src, insert_at.min(src.len()));
    let comment = format!(
        "{indent}// canvas:collapse span={}..{} title={}\n",
        start,
        end,
        quoted_attr(title)
    );
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: insert_at,
                end: insert_at,
            },
            &comment,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas collapse insert overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn extract_inline_candidate(
    path: &Path,
    src: &str,
    inline_id: &str,
    function: &str,
    ret_type: &str,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(inline) = projection.inline_exprs.iter().find(|i| i.id == inline_id) else {
        return Err(edit_error(
            "not_found",
            "Canvas inline expression no longer exists",
        ));
    };
    let expr = snippet(src, Span::new(inline.span.start, inline.span.end));
    if expr.contains("#Unsafe") {
        return Err(edit_error(
            "diagnostic",
            "Error [E2203]: Canvas extract cannot cross an unsafe source span",
        ));
    }
    let params = extract_params(&projection.json, &expr);
    let signature = params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    let args = params
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let helper = format!("fn {function}({signature}) -> {ret_type} {{\n    return {expr}\n}}\n\n");
    let replacement = format!("{function}({args})");
    FixEngine::apply_edits(
        src,
        &[
            edit(SourceSpan { start: 0, end: 0 }, &helper),
            edit(inline.span, &replacement),
        ],
    )
    .map_err(|_| edit_error("overlap", "Canvas extract edits overlapped"))
}

fn inline_helper_candidate(
    path: &Path,
    src: &str,
    inline_id: Option<&str>,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let span = if let Some(inline_id) = inline_id {
        let Some(inline) = projection.inline_exprs.iter().find(|i| i.id == inline_id) else {
            return Err(edit_error(
                "not_found",
                "Canvas inline expression no longer exists",
            ));
        };
        inline.span
    } else {
        SourceSpan {
            start: start.unwrap_or(0),
            end: end.unwrap_or(0),
        }
    };
    let call = snippet(src, Span::new(span.start, span.end));
    let Some((name, args)) = parse_simple_call(&call) else {
        return Err(edit_error(
            "diagnostic",
            "Canvas inline needs a direct helper call expression",
        ));
    };
    let Some((params, body)) = find_simple_helper(src, &name) else {
        return Err(edit_error(
            "diagnostic",
            "Canvas inline could not find a simple helper return body",
        ));
    };
    if params.len() != args.len() {
        return Err(edit_error(
            "diagnostic",
            "Canvas inline helper argument count changed",
        ));
    }
    let mut expr = body;
    for (param, arg) in params.iter().zip(args.iter()) {
        expr = replace_ident(&expr, param, arg);
    }
    FixEngine::apply_edits(src, &[edit(span, &expr)])
        .map_err(|_| edit_error("overlap", "Canvas inline helper edit overlapped"))
}

fn write_checked_formatted(path: &Path, before: &str, candidate: &str) -> Result<String, String> {
    let formatted = crate::format_source(candidate)
        .map_err(|diags| diagnostics_error(path, candidate, &diags))?;
    let path_str = path.to_string_lossy();
    let abs = canonical_path(path);
    let (diags, _, _) =
        crate::Driver::check_file_with_effect_facts(&path_str, Some((&abs, &formatted)), true);
    let errors: Vec<Diagnostic> = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(diagnostics_error(path, &formatted, &errors));
    }
    let changed = formatted != before;
    if changed {
        fs::write(path, formatted).map_err(|e| edit_error("io", &e.to_string()))?;
    }
    Ok(edit_ok(changed, path))
}

fn graph_id(module_display: &str, f: &AST::Func) -> String {
    let file = Path::new(module_display)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(module_display);
    format!(
        "fn:{file}::{}@{}-{}",
        f.name, f.name_span.start, f.name_span.end
    )
}

fn graph_id_name_span(graph_id: &str) -> Option<SourceSpan> {
    let (_, range) = graph_id.rsplit_once('@')?;
    let (start, end) = range.split_once('-')?;
    Some(SourceSpan {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    })
}

fn function_signature_span(src: &str, name_span: SourceSpan) -> Result<SourceSpan, String> {
    let start = line_start(src, name_span.start);
    let Some(brace_rel) = src[name_span.end.min(src.len())..].find('{') else {
        return Err(edit_error(
            "not_found",
            "Canvas function signature no longer has a body",
        ));
    };
    let mut end = name_span.end + brace_rel;
    while end > start && src.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    Ok(SourceSpan { start, end })
}

fn insert_offset(src: &str, f: &AST::Func) -> usize {
    if let Some(first) = f.body.first() {
        return line_start(src, first.span().start);
    }
    line_after(src, f.name_span.end)
}

fn line_start(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

fn line_after(src: &str, offset: usize) -> usize {
    src[offset.min(src.len())..]
        .find('\n')
        .map(|i| offset + i + 1)
        .unwrap_or(src.len())
}

fn indentation_at(src: &str, offset: usize) -> String {
    let start = line_start(src, offset);
    src[start..offset.min(src.len())]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

fn assignment_title(target: &AST::LValue, op: Option<AST::BinOp>) -> String {
    let op = op
        .map(|op| format!("{op:?}="))
        .unwrap_or_else(|| "=".to_string());
    match target {
        AST::LValue::Local { name, .. } => format!("{name} {op}"),
        AST::LValue::Index { .. } => format!("index {op}"),
        AST::LValue::Field { field, .. } => format!(".{field} {op}"),
    }
}

fn lvalue_type(g: &GraphBuilder, target: &AST::LValue) -> String {
    match target {
        AST::LValue::Local { name, .. } => g
            .local_types
            .get(name)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        AST::LValue::Index { .. } | AST::LValue::Field { .. } => "unknown".to_string(),
    }
}

fn expr_title(expr: &Expr) -> &'static str {
    match expr {
        Expr::StructLit { .. } => "construct",
        Expr::EnumLit { .. } => "variant",
        Expr::Lambda(_) => "lambda",
        Expr::If { .. } => "if expression",
        Expr::ListLit(_, _) => "list",
        Expr::MapLit(_, _) => "map",
        Expr::TupleLit(_, _, _) => "tuple",
        Expr::Index { .. } => "index",
        Expr::Slice { .. } => "slice",
        Expr::CallValue { .. } => "call value",
        Expr::FanOut { .. } => "fanout",
        Expr::Deref(_, _) | Expr::RawOf(_, _) | Expr::PtrFromAddr { .. } => "unsafe expr",
        Expr::OrFallback { .. } => "fallback",
        Expr::PatternTest { .. } => "pattern test",
        Expr::Try(_, _, _) => "fallible",
        _ => "expression",
    }
}

fn starts_uppercase(s: &str) -> bool {
    s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}

fn binding_type(g: &GraphBuilder, name: &str, b: &AST::Binding) -> String {
    b.ty.as_ref()
        .map(AST::Type::name)
        .or_else(|| g.local_types.get(name).cloned())
        .unwrap_or_else(|| "unknown".to_string())
}

fn expr_type(g: &GraphBuilder, index: &SemIndex, expr: &Expr) -> String {
    match expr {
        Expr::Int(_, _, _) => "Int".to_string(),
        Expr::Float(_, _, is_f32) => if *is_f32 { "F32" } else { "Float" }.to_string(),
        Expr::Bool(_, _) => "Bool".to_string(),
        Expr::Str(_, _) => "String".to_string(),
        Expr::Char(_, _) => "Char".to_string(),
        Expr::Ident(name, _) => g
            .local_types
            .get(name)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        Expr::Binary(op, left, _, _) => {
            if op_is_bool(*op) {
                "Bool".to_string()
            } else {
                expr_type(g, index, left)
            }
        }
        Expr::Call(c) => call_ret(index, &c.name).unwrap_or_else(|| "unknown".to_string()),
        Expr::MethodCall { resolved_ret, .. } => resolved_ret
            .as_ref()
            .map(AST::Type::name)
            .unwrap_or_else(|| "unknown".to_string()),
        Expr::Try(inner, _, _) => expr_type(g, index, inner).trim_end_matches('?').to_string(),
        _ => "unknown".to_string(),
    }
}

fn op_is_bool(op: AST::BinOp) -> bool {
    matches!(
        op,
        AST::BinOp::Eq
            | AST::BinOp::Ne
            | AST::BinOp::Lt
            | AST::BinOp::Le
            | AST::BinOp::Gt
            | AST::BinOp::Ge
            | AST::BinOp::And
            | AST::BinOp::Or
    )
}

fn call_ret(index: &SemIndex, name: &str) -> Option<String> {
    index.definitions().iter().find_map(|d| {
        if d.name == name {
            if let SymbolKind::Function { ret, .. } = &d.kind {
                return ret.clone();
            }
        }
        None
    })
}

fn effect_badges(index: &SemIndex, function: &str) -> Vec<&'static str> {
    if let Some(effects) = index.effect_of(function) {
        if !effects.direct.is_empty() || !effects.inferred.is_empty() {
            return vec!["effects"];
        }
    }
    Vec::new()
}

fn pure_leaf(expr: &Expr) -> bool {
    match expr {
        Expr::Str(_, _)
        | Expr::Int(_, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::Field(_, _, _)
        | Expr::OptField { .. }
        | Expr::Absent(_)
        | Expr::Todo { .. } => true,
        Expr::Unary(_, inner, _) | Expr::Paren(inner, _) | Expr::Copy(inner, _) => pure_leaf(inner),
        Expr::Binary(_, left, right, _) => pure_leaf(left) && pure_leaf(right),
        Expr::CompareChain { operands, .. } => operands.iter().all(pure_leaf),
        Expr::ListLit(items, _) => items.iter().all(pure_leaf),
        Expr::MapLit(items, _) => items.iter().all(|(k, v)| pure_leaf(k) && pure_leaf(v)),
        Expr::TupleLit(items, _, _) => items.iter().all(|(_, e)| pure_leaf(e)),
        Expr::Ok(inner, _) | Expr::Err(inner, _) | Expr::Present(inner, _) => pure_leaf(inner),
        _ => false,
    }
}

fn wire_ident_refs(g: &mut GraphBuilder, expr: &Expr, input_pin: &str) {
    if let Expr::Ident(name, span) = expr {
        if let Some(out) = g.local_pins.get(name).cloned() {
            add_wire_with_span(g, &out, input_pin, "data", Some((*span).into()));
        }
    }
}

fn snippet(src: &str, span: Span) -> String {
    src.get(span.start..span.end)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn edit(span: SourceSpan, text: &str) -> TextEdit {
    TextEdit {
        span: Span::new(span.start, span.end),
        new_text: text.to_string(),
    }
}

fn edit_ok(changed: bool, path: &Path) -> String {
    let src = fs::read_to_string(path).unwrap_or_default();
    format!(
        "{{\"protocol\":\"jet.canvas.edit\",\"schema_version\":{},\"changed\":{},\"revision\":{},\"source_text\":{}}}",
        EDIT_SCHEMA_VERSION,
        if changed { "true" } else { "false" },
        json_str(&source_revision(&src)),
        json_str(&src)
    )
}

fn preview_ok(before: &str, after: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.preview\",\"schema_version\":{},\"changed\":{},\"diff\":{},\"after_revision\":{}}}",
        EDIT_SCHEMA_VERSION,
        if before != after { "true" } else { "false" },
        json_str(&simple_diff(before, after)),
        json_str(&source_revision(after))
    )
}

fn canvas_action_preview_ok(before: &str, after: &str, action_id: &str, callee: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.action\",\"schema_version\":{},\"ok\":true,\"changed\":{},\"action_id\":{},\"callee\":{},\"engine\":\"checked-tir+jit\",\"execution\":\"preview\",\"writes\":\"source_transaction_only\",\"authority\":[\"canvas.source_edit:current_file\"],\"audit\":{{\"package_id\":\"local-source\",\"version\":\"workspace\",\"hash\":{},\"touched_files\":[\"current_file\"],\"diagnostics\":[]}},\"diff\":{},\"after_revision\":{}}}",
        ACTION_SCHEMA_VERSION,
        if before != after { "true" } else { "false" },
        json_str(action_id),
        json_str(callee),
        json_str(&source_revision(after)),
        json_str(&simple_diff(before, after)),
        json_str(&source_revision(after))
    )
}

fn query_ok(op: &str, src: &str, results_json: &str, extra_fields: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.query\",\"schema_version\":{},\"ok\":true,\"op\":{},\"revision\":{},\"results\":{},{} }}",
        QUERY_SCHEMA_VERSION,
        json_str(op),
        json_str(&source_revision(src)),
        results_json,
        extra_fields
    )
}

fn query_result_json(
    kind: &str,
    title: &str,
    symbol: &str,
    module_path: &str,
    span: SourceSpan,
    node: Option<&NodeQueryRef>,
    src: &str,
) -> String {
    let (graph_id, node_id, node_kind) = match node {
        Some(n) => (n.graph_id.as_str(), n.node_id.as_str(), n.kind.as_str()),
        None => ("", "", ""),
    };
    format!(
        "{{\"kind\":{},\"title\":{},\"symbol\":{},\"module_path\":{},\"graph_id\":{},\"node_id\":{},\"node_kind\":{},\"source_span\":{},\"line\":{},\"excerpt\":{}}}",
        json_str(kind),
        json_str(title),
        json_str(symbol),
        json_str(module_path),
        json_str(graph_id),
        json_str(node_id),
        json_str(node_kind),
        span_json(span),
        line_number_for(src, span.start),
        json_str(&source_line_for(src, span.start))
    )
}

fn open_query_context(path: &Path, src: &str) -> Result<(Projection, SemIndex), String> {
    let projection =
        project_file(path).map_err(|diags| query_diagnostics_error(path, src, &diags))?;
    let index = jet_semindex::open(path).map_err(|e| query_error("check", &e.to_string()))?;
    Ok((projection, index))
}

fn node_for_span(projection: &Projection, span: SourceSpan) -> Option<NodeQueryRef> {
    projection
        .node_refs
        .iter()
        .filter(|node| spans_overlap(node.span, span))
        .min_by_key(|node| node.span.end.saturating_sub(node.span.start))
        .cloned()
}

fn nearest_node(projection: &Projection, offset: usize) -> Option<NodeQueryRef> {
    projection
        .node_refs
        .iter()
        .min_by_key(|node| {
            if offset < node.span.start {
                node.span.start - offset
            } else {
                offset.saturating_sub(node.span.end)
            }
        })
        .cloned()
}

fn spans_overlap(a: SourceSpan, b: SourceSpan) -> bool {
    a.start <= b.end && b.start <= a.end
}

fn contains_ci(text: &str, needle: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn text_matches(src: &str, needle: &str) -> Vec<SourceSpan> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let hay = src.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(pos) = hay[offset..].find(&needle) {
        let start = offset + pos;
        let end = start + needle.len();
        out.push(SourceSpan { start, end });
        offset = end;
    }
    out
}

fn dedupe_results(results: &mut Vec<String>) {
    let mut seen = Vec::<String>::new();
    results.retain(|r| {
        if seen.iter().any(|s| s == r) {
            false
        } else {
            seen.push(r.clone());
            true
        }
    });
}

fn line_number_for(src: &str, offset: usize) -> usize {
    let offset = floor_char_boundary(src, offset);
    src[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

fn source_line_for(src: &str, offset: usize) -> String {
    let offset = floor_char_boundary(src, offset);
    let start = src[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(src.len());
    src[start..end].trim().to_string()
}

fn floor_char_boundary(src: &str, mut offset: usize) -> usize {
    offset = offset.min(src.len());
    while offset > 0 && !src.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn simple_diff(before: &str, after: &str) -> String {
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let mut out = String::from("--- before\n+++ after\n");
    for line in &before_lines {
        if !after_lines.contains(line) {
            out.push('-');
            out.push_str(line);
            out.push('\n');
        }
    }
    for line in &after_lines {
        if !before_lines.contains(line) {
            out.push('+');
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn edit_error(kind: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.edit\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        EDIT_SCHEMA_VERSION,
        json_str(kind),
        json_str(message)
    )
}

fn query_error(kind: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.query\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        QUERY_SCHEMA_VERSION,
        json_str(kind),
        json_str(message)
    )
}

fn diagnostics_error(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    edit_error(
        "diagnostic",
        &crate::render_diagnostics(&path.display().to_string(), src, diags),
    )
}

fn query_diagnostics_error(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    query_error(
        "diagnostic",
        &crate::render_diagnostics(&path.display().to_string(), src, diags),
    )
}

fn debug_ok(
    src: &str,
    graph_json: &str,
    transcript: &str,
    breakpoint_lines: &[usize],
    watches: &[String],
) -> String {
    let active_line = active_line_from_transcript(transcript);
    let active_span = active_line
        .map(|line| line_span(src, line))
        .unwrap_or(SourceSpan { start: 0, end: 0 });
    let active_node = active_line
        .and_then(|_| record_id_for_span(graph_json, "node_id", active_span))
        .unwrap_or_default();
    let active_wire = active_line
        .and_then(|_| record_id_for_span(graph_json, "wire_id", active_span))
        .unwrap_or_default();
    let active_graph = graph_id_from_node_id(&active_node).unwrap_or_default();
    let overlay = if active_line.is_some() {
        "running"
    } else {
        "finished"
    };
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":true,\"revision\":{},\"session\":{{\"id\":\"local-source-span\",\"state\":{},\"persistence\":\"local-source-span\"}},\"overlay\":{{\"debug_overlay\":{},\"active_line\":{},\"active_span\":{},\"active_graph_id\":{},\"active_node_id\":{},\"active_wire_id\":{},\"breakpoints\":[{}],\"locals\":[{}],\"watches\":[{}],\"call_stack\":[{}],\"trace\":[{}]}}}}",
        DEBUG_SCHEMA_VERSION,
        json_str(&source_revision(src)),
        json_str(overlay),
        json_str(overlay),
        active_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "null".to_string()),
        span_json(active_span),
        json_str(&active_graph),
        json_str(&active_node),
        json_str(&active_wire),
        breakpoint_json(src, breakpoint_lines),
        locals_json(transcript),
        watches_json(transcript, watches),
        call_stack_json(transcript),
        trace_json(transcript)
    )
}

fn debug_error(kind: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        DEBUG_SCHEMA_VERSION,
        json_str(kind),
        json_str(message)
    )
}

fn debug_diagnostics_error(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    debug_error(
        "diagnostic",
        &crate::render_diagnostics(&path.display().to_string(), src, diags),
    )
}

fn required_debug_string(text: &str, key: &str) -> Result<String, String> {
    json_string_field(text, key)
        .ok_or_else(|| debug_error("bad_request", &format!("missing `{key}`")))
}

fn active_line_from_transcript(transcript: &str) -> Option<usize> {
    for line in transcript.lines().rev() {
        if !line.contains("<- here") {
            continue;
        }
        let before_pipe = line.split('|').next()?.trim();
        if let Some(n) = before_pipe.split_whitespace().last() {
            if let Ok(line) = n.parse::<usize>() {
                return Some(line);
            }
        }
    }
    for line in transcript.lines().rev() {
        if let Some((_, rest)) = line.split_once("breakpoint hit") {
            if let Some((before_in, _)) = rest.split_once("  in ") {
                if let Some((_, line_no)) = before_in.trim().rsplit_once(':') {
                    if let Ok(line) = line_no.parse::<usize>() {
                        return Some(line);
                    }
                }
            }
        }
    }
    None
}

fn line_span(src: &str, line: usize) -> SourceSpan {
    let mut current = 1usize;
    let mut start = 0usize;
    for (i, ch) in src.char_indices() {
        if current == line {
            start = i;
            break;
        }
        if ch == '\n' {
            current += 1;
        }
    }
    if line > current {
        start = src.len();
    }
    let end = src[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(src.len());
    SourceSpan { start, end }
}

fn line_of_offset(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

fn line_from_anchor(src: &str, anchor: &str) -> Option<usize> {
    let (start, _) = anchor.split_once(':')?;
    let offset = start.parse::<usize>().ok()?;
    Some(line_of_offset(src, offset))
}

fn record_id_for_span(json: &str, id_key: &str, active: SourceSpan) -> Option<String> {
    let needle = format!("\"{id_key}\":");
    let mut best: Option<(usize, String)> = None;
    for chunk in json.split(&needle).skip(1) {
        let Some((id, _)) = parse_json_string(chunk.trim_start()) else {
            continue;
        };
        let Some(pos) = chunk.find("\"source_span\"") else {
            continue;
        };
        let rest = &chunk[pos + "\"source_span\"".len()..];
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let value = rest[colon + 1..].trim_start();
        if value.starts_with("null") {
            continue;
        }
        let Some(start) = json_usize_field(value, "start") else {
            continue;
        };
        let Some(end) = json_usize_field(value, "end") else {
            continue;
        };
        if span_overlaps(SourceSpan { start, end }, active) {
            let width = end.saturating_sub(start);
            if best.as_ref().map(|(w, _)| width < *w).unwrap_or(true) {
                best = Some((width, id));
            }
        }
    }
    best.map(|(_, id)| id)
}

fn span_overlaps(a: SourceSpan, b: SourceSpan) -> bool {
    a.start <= b.end && b.start <= a.end
}

fn graph_id_from_node_id(node_id: &str) -> Option<String> {
    for marker in [":entry", ":stmt:", ":expr:"] {
        if let Some(pos) = node_id.find(marker) {
            return Some(node_id[..pos].to_string());
        }
    }
    None
}

fn breakpoint_json(src: &str, lines: &[usize]) -> String {
    lines
        .iter()
        .map(|line| {
            let span = line_span(src, *line);
            format!(
                "{{\"line\":{},\"source_span\":{},\"state\":\"valid\"}}",
                line,
                span_json(span)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn locals_json(transcript: &str) -> String {
    let Some(line) = transcript
        .lines()
        .rev()
        .find(|line| line.starts_with("locals:"))
    else {
        return String::new();
    };
    parse_assignments(line.trim_start_matches("locals:").trim())
}

fn watches_json(transcript: &str, watches: &[String]) -> String {
    watches
        .iter()
        .filter_map(|watch| {
            let prefix = format!("{watch} = ");
            transcript
                .lines()
                .rev()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(|value| {
                    format!(
                        "{{\"name\":{},\"value\":{},\"state\":\"ok\"}}",
                        json_str(watch),
                        json_str(value)
                    )
                })
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_assignments(text: &str) -> String {
    if text == "(none)" || text.is_empty() {
        return String::new();
    }
    text.split("   ")
        .filter_map(|part| {
            let (name, value) = part.split_once(" = ")?;
            Some(format!(
                "{{\"name\":{},\"value\":{}}}",
                json_str(name.trim()),
                json_str(value.trim())
            ))
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn call_stack_json(transcript: &str) -> String {
    transcript
        .lines()
        .filter(|line| line.starts_with('#') && line.contains(" at "))
        .map(json_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn trace_json(transcript: &str) -> String {
    transcript
        .lines()
        .map(json_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn canonical_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn git_root(path: &Path) -> Option<PathBuf> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let out = Command::new("git")
        .args(["-C"])
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

fn git_relative_path(root: &Path, path: &Path) -> String {
    let abs = canonical_path(path);
    let root = canonical_path(root);
    abs.strip_prefix(&root)
        .unwrap_or(&abs)
        .to_string_lossy()
        .replace('\\', "/")
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

fn untracked_diff(rel: &str, src: &str) -> String {
    let mut diff = format!("--- /dev/null\n+++ b/{rel}\n");
    for line in src.lines() {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

fn validate_ident(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(edit_error("bad_request", "empty identifier"));
    };
    if (!first.is_ascii_alphabetic() && first != '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(edit_error("bad_request", "identifier is not a Jet name"));
    }
    Ok(())
}

fn validate_query_ident(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(query_error("bad_request", "empty identifier"));
    };
    if (!first.is_ascii_alphabetic() && first != '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(query_error("bad_request", "identifier is not a Jet name"));
    }
    Ok(())
}

fn validate_qualified_name(name: &str) -> Result<(), String> {
    for part in name.split('.') {
        validate_ident(part)?;
    }
    Ok(())
}

fn validate_signature_fragment(fragment: &str) -> Result<(), String> {
    if fragment.contains('{') || fragment.contains('}') || fragment.contains('\n') {
        return Err(edit_error(
            "bad_request",
            "function parameter text must stay inside the signature",
        ));
    }
    Ok(())
}

fn validate_type_fragment(fragment: &str) -> Result<(), String> {
    if fragment.trim().is_empty()
        || fragment.contains('{')
        || fragment.contains('}')
        || fragment.contains('\n')
    {
        return Err(edit_error("bad_request", "return type is not a Jet type"));
    }
    Ok(())
}

fn validate_function_signature(signature: &str) -> Result<(), String> {
    if signature.contains('{') || signature.contains('}') || signature.contains('\n') {
        return Err(edit_error(
            "bad_request",
            "function signature must not include a body",
        ));
    }
    if !signature.split_whitespace().any(|part| part == "fn") {
        return Err(edit_error(
            "bad_request",
            "function signature must include fn",
        ));
    }
    Ok(())
}

fn validate_comment_color(color: &str) -> Result<(), String> {
    let ok = color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(edit_error(
            "bad_request",
            "Canvas comment color must be #RRGGBB",
        ))
    }
}

fn validate_comment_alpha(alpha: &str) -> Result<(), String> {
    let Ok(value) = alpha.parse::<f32>() else {
        return Err(edit_error(
            "bad_request",
            "Canvas comment alpha must be a number",
        ));
    };
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(edit_error(
            "bad_request",
            "Canvas comment alpha must be between 0 and 1",
        ))
    }
}

fn normalize_bounds(bounds: &str) -> Result<String, String> {
    let nums = bounds
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(|part| part.trim().parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| edit_error("bad_request", "Canvas comment bounds must be x,y,w,h"))?;
    if nums.len() != 4 || nums[2] <= 0 || nums[3] <= 0 {
        return Err(edit_error(
            "bad_request",
            "Canvas comment bounds must be x,y,w,h with positive size",
        ));
    }
    Ok(format!("{},{},{},{}", nums[0], nums[1], nums[2], nums[3]))
}

fn quoted_attr(value: &str) -> String {
    json_str(value)
}

fn find_comment_hint(src: &str, graph_json: &str, region_id: &str) -> Option<CommentHint> {
    for chunk in graph_json.split("\"region_id\":").skip(1) {
        let (id, _) = parse_json_string(chunk.trim_start())?;
        if id != region_id {
            continue;
        }
        let start = json_usize_field(chunk, "start")?;
        let end = json_usize_field(chunk, "end")?;
        return canvas_comment_hints(src)
            .into_iter()
            .find(|hint| hint.anchor.start == start && hint.anchor.end == end);
    }
    None
}

fn find_hint_region(
    src: &str,
    graph_json: &str,
    region_id: &str,
    kind: &str,
) -> Option<CommentHint> {
    for chunk in graph_json.split("\"region_id\":").skip(1) {
        let (id, _) = parse_json_string(chunk.trim_start())?;
        if id != region_id {
            continue;
        }
        if json_string_field(chunk, "kind").as_deref() != Some(kind) {
            continue;
        }
        let start = json_usize_field(chunk, "start")?;
        let end = json_usize_field(chunk, "end")?;
        let hints = match kind {
            "comment" => canvas_comment_hints(src),
            "collapse" => canvas_collapse_hints(src),
            _ => return None,
        };
        return hints
            .into_iter()
            .find(|hint| hint.anchor.start == start && hint.anchor.end == end);
    }
    None
}

fn extract_params(graph_json: &str, expr: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for ident in identifiers(expr) {
        if let Some(ty) = graph_type_for_name(graph_json, &ident) {
            if !out.iter().any(|(name, _)| name == &ident) {
                out.push((ident, ty));
            }
        }
    }
    out
}

fn graph_type_for_name(graph_json: &str, name: &str) -> Option<String> {
    for chunk in graph_json.split("\"name\":").skip(1) {
        let (found, _) = parse_json_string(chunk.trim_start())?;
        if found != name || !chunk.contains("\"direction\":\"output\"") {
            continue;
        }
        let pos = chunk.find("\"type\"")?;
        let rest = &chunk[pos + "\"type\"".len()..];
        let colon = rest.find(':')?;
        return parse_json_string(rest[colon + 1..].trim_start()).map(|(s, _)| s);
    }
    None
}

fn identifiers(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in expr.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            current.push(c);
        } else if !current.is_empty() {
            if current
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false)
                && !matches!(current.as_str(), "true" | "false" | "ok" | "err")
            {
                out.push(current.clone());
            }
            current.clear();
        }
    }
    out
}

fn parse_simple_call(call: &str) -> Option<(String, Vec<String>)> {
    let open = call.find('(')?;
    let close = call.rfind(')')?;
    let name = call[..open].trim();
    validate_ident(name).ok()?;
    let args = if call[open + 1..close].trim().is_empty() {
        Vec::new()
    } else {
        call[open + 1..close]
            .split(',')
            .map(|arg| arg.trim().to_string())
            .collect()
    };
    Some((name.to_string(), args))
}

fn find_simple_helper(src: &str, name: &str) -> Option<(Vec<String>, String)> {
    let needle = format!("fn {name}(");
    let start = src.find(&needle)?;
    let params_start = start + needle.len();
    let params_end = src[params_start..].find(')')? + params_start;
    let params = src[params_start..params_end]
        .split(',')
        .filter_map(|param| {
            param
                .trim()
                .split_once(':')
                .map(|(name, _)| name.trim().to_string())
        })
        .collect::<Vec<_>>();
    let body_start = src[params_end..].find('{')? + params_end + 1;
    let body_end = src[body_start..].find('}')? + body_start;
    let body = src[body_start..body_end].trim();
    let returned = body.strip_prefix("return ")?;
    Some((params, returned.trim().to_string()))
}

fn replace_ident(expr: &str, ident: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut current = String::new();
    for c in expr.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            current.push(c);
        } else {
            if !current.is_empty() {
                if current == ident {
                    out.push_str(replacement);
                } else {
                    out.push_str(&current);
                }
                current.clear();
            }
            out.push(c);
        }
    }
    out.trim_end().to_string()
}

fn attr_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let pos = text.find(&needle)?;
    let rest = text[pos + needle.len()..].trim_start();
    if rest.starts_with('"') {
        return parse_json_string(rest).map(|(s, _)| s);
    }
    Some(
        rest.chars()
            .take_while(|c| !c.is_whitespace())
            .collect::<String>(),
    )
}

fn attr_span(text: &str, key: &str) -> Option<SourceSpan> {
    let raw = attr_string(text, key)?;
    let (start, end) = raw.split_once("..")?;
    Some(SourceSpan {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    })
}

fn attr_bounds(text: &str, key: &str) -> Option<(i32, i32, i32, i32)> {
    let needle = format!("{key}=");
    let pos = text.find(&needle)?;
    let rest = text[pos + needle.len()..].trim_start();
    let rest = rest.strip_prefix('(')?;
    let close = rest.find(')')?;
    let nums = rest[..close]
        .split(',')
        .map(|part| part.trim().parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if nums.len() == 4 {
        Some((nums[0], nums[1], nums[2], nums[3]))
    } else {
        None
    }
}

fn required_string(text: &str, key: &str) -> Result<String, String> {
    json_string_field(text, key)
        .ok_or_else(|| edit_error("bad_request", &format!("missing `{key}`")))
}

fn required_query_string(text: &str, key: &str) -> Result<String, String> {
    json_string_field(text, key)
        .ok_or_else(|| query_error("bad_request", &format!("missing `{key}`")))
}

fn json_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = text.find(&needle)?;
    let rest = &text[pos + needle.len()..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    parse_json_string(rest.trim_start()).map(|(s, _)| s)
}

fn json_usize_field(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let pos = text.find(&needle)?;
    let rest = &text[pos + needle.len()..];
    let colon = rest.find(':')?;
    rest[colon + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn json_usize_array(text: &str, key: &str) -> Vec<usize> {
    let needle = format!("\"{key}\"");
    let Some(pos) = text.find(&needle) else {
        return Vec::new();
    };
    let rest = &text[pos + needle.len()..];
    let Some(colon) = rest.find(':') else {
        return Vec::new();
    };
    let rest = rest[colon + 1..].trim_start();
    let Some(mut rest) = rest.strip_prefix('[') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.starts_with(']') {
            break;
        }
        let digits = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() {
            break;
        }
        if let Ok(n) = digits.parse::<usize>() {
            out.push(n);
        }
        rest = &rest[digits.len()..];
        rest = rest.trim_start();
        if rest.starts_with(',') {
            rest = &rest[1..];
        }
    }
    out
}

fn wire_span_from_json_chunk(chunk: &str, wire_id: &str) -> Option<SourceSpan> {
    let (id, _) = parse_json_string(chunk.trim_start())?;
    if id != wire_id {
        return None;
    }
    let pos = chunk.find("\"source_span\"")?;
    let rest = &chunk[pos + "\"source_span\"".len()..];
    let colon = rest.find(':')?;
    let value = rest[colon + 1..].trim_start();
    if value.starts_with("null") {
        return None;
    }
    Some(SourceSpan {
        start: json_usize_field(value, "start")?,
        end: json_usize_field(value, "end")?,
    })
}

fn json_string_array(text: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let Some(pos) = text.find(&needle) else {
        return Vec::new();
    };
    let rest = &text[pos + needle.len()..];
    let Some(colon) = rest.find(':') else {
        return Vec::new();
    };
    let rest = rest[colon + 1..].trim_start();
    let Some(mut rest) = rest.strip_prefix('[') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.starts_with(']') {
            break;
        }
        let Some((value, consumed)) = parse_json_string(rest) else {
            break;
        };
        out.push(value);
        rest = &rest[consumed..];
        rest = rest.trim_start();
        if rest.starts_with(',') {
            rest = &rest[1..];
        }
    }
    out
}

fn parse_json_string(text: &str) -> Option<(String, usize)> {
    let mut chars = text.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (i, c) in chars {
        if escaped {
            out.push(match c {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some((out, i + 1));
        } else {
            out.push(c);
        }
    }
    None
}

fn span_json(span: SourceSpan) -> String {
    format!("{{\"start\":{},\"end\":{}}}", span.start, span.end)
}

fn json_strs(values: &[String]) -> String {
    values
        .iter()
        .map(|s| json_str(s))
        .collect::<Vec<_>>()
        .join(",")
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
