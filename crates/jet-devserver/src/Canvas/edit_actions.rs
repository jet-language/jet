use std::fs;
use std::path::{Path, PathBuf};

use jet_driver::Diagnostics::{Diagnostic, Severity, Span, TextEdit};
use jet_driver::AST::{self, Expr, Item, Stmt};
use jet_driver::FixEngine;
use jet_semindex::SourceSpan;

use super::debug_source_git::canonical_path;
use super::graph_helpers::{
    diagnostics_error, edit, edit_error, edit_ok, function_signature_span, graph_id_name_span,
    indentation_at, line_after, line_start, snippet,
};
use super::graph_json::func_source_span;
use super::graph_projection::trait_method_signature;
use super::project_scan::project_file;
use super::query_actions::{core_member_params, default_arg_for_type};
use super::validation_json::{
    extract_params, find_comment_hint, find_hint_region, find_simple_helper, normalize_bounds,
    parse_simple_call, quoted_attr, replace_ident, validate_comment_alpha, validate_comment_color,
    wire_span_from_json_chunk,
};

pub(super) fn apply_noop(path: &Path, src: &str) -> Result<String, String> {
    let formatted =
        jet_driver::Formatter::format_source(src).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let changed = formatted != src;
    if changed {
        fs::write(path, formatted).map_err(|e| edit_error("io", &e.to_string()))?;
    }
    Ok(edit_ok(changed, path))
}

pub(super) fn apply_rename(path: &Path, src: &str, from: &str, to: &str) -> Result<String, String> {
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

pub(super) fn apply_create_function(
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

pub(super) fn apply_create_trait_impl(
    path: &Path,
    src: &str,
    type_name: &str,
    trait_name: &str,
) -> Result<String, String> {
    let path_str = path.to_string_lossy();
    let (diags, bundle) = jet_driver::Driver::check_file(&path_str, None, true);
    let errors = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(diagnostics_error(path, src, &errors));
    }
    let Some(bundle) = bundle else {
        return Err(edit_error(
            "check",
            "Canvas could not read checked trait facts",
        ));
    };
    let Some(trait_def) = find_trait_def(&bundle, trait_name) else {
        return Err(edit_error(
            "not_found",
            "Canvas trait was not found in source",
        ));
    };
    let mut body = String::new();
    for method in &trait_def.methods {
        let sig = trait_method_signature(method);
        body.push_str("    ");
        body.push_str(&sig);
        body.push_str(" {\n");
        if let Some(ret) = &method.return_type {
            if ret.name() != "Void" {
                body.push_str("        return ");
                body.push_str(&default_arg_for_type(&ret.name()));
                body.push('\n');
            }
        }
        body.push_str("    }\n");
    }
    let impl_block = format!("\nimpl {type_name}.{trait_name} {{\n{body}}}\n");
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: src.len(),
                end: src.len(),
            },
            &impl_block,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas trait impl edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn find_trait_def<'a>(
    bundle: &'a AST::ProgramBundle,
    trait_name: &str,
) -> Option<&'a AST::TraitDef> {
    fn find_in_items<'a>(items: &'a [Item], trait_name: &str) -> Option<&'a AST::TraitDef> {
        for item in items {
            match item {
                Item::Trait(t) if t.name == trait_name => return Some(t),
                Item::CodeModule(m) => {
                    if let Some(body) = &m.body {
                        if let Some(t) = find_in_items(body, trait_name) {
                            return Some(t);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    for module in &bundle.modules {
        if let Some(t) = find_in_items(&module.items, trait_name) {
            return Some(t);
        }
    }
    None
}

pub(super) fn apply_edit_function_signature(
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

pub(super) fn apply_inline_edit(
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

pub(super) fn apply_promote_inline(
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

pub(super) fn apply_visible_conversion(
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

pub(super) fn apply_break_link(path: &Path, src: &str, wire_id: &str) -> Result<String, String> {
    rewrite_wire_expr(path, src, wire_id, "#Todo")
}

pub(super) fn apply_move_link(
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

pub(super) fn apply_insert_call(
    path: &Path,
    src: &str,
    graph_id: &str,
    callee: &str,
    args: &[String],
    bind: Option<&str>,
    wire_inline_expr_id: Option<&str>,
    wire_expr: Option<&str>,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let final_args = if args.is_empty() {
        wire_expr.map(|expr| vec![expr.to_string()]).unwrap_or_default()
    } else {
        args.to_vec()
    };
    let plan = insert_call_plan(src, callee, &final_args);
    let call = if plan.fallible {
        format!(
            "{}({}) ?? panic(\"canvas\")",
            plan.callee,
            plan.args.join(", ")
        )
    } else {
        format!("{}({})", plan.callee, plan.args.join(", "))
    };
    if let Some(inline_id) = wire_inline_expr_id {
        let Some(inline) = projection.inline_exprs.iter().find(|i| i.id == inline_id) else {
            return Err(edit_error(
                "not_found",
                "Canvas inline expression no longer exists",
            ));
        };
        let mut edits = vec![edit(inline.span, &call)];
        if let Some(import) = plan.import {
            edits.push(import);
        }
        let changed = FixEngine::apply_edits(src, &edits)
            .map_err(|_| edit_error("overlap", "Canvas wired insert edit overlapped"))?;
        return write_checked_formatted(path, src, &changed);
    }
    let Some(anchor) = projection
        .graph_anchors
        .iter()
        .find(|a| a.graph_id == graph_id)
    else {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    };
    let stmt = match bind {
        Some(name) => format!("    {name} :: {call}\n"),
        None => format!("    {call}\n"),
    };
    let mut edits = vec![edit(
        SourceSpan {
            start: anchor.insert_offset,
            end: anchor.insert_offset,
        },
        &stmt,
    )];
    if let Some(import) = plan.import {
        edits.push(import);
    }
    let changed = FixEngine::apply_edits(
        src,
        &edits,
    )
    .map_err(|_| edit_error("overlap", "Canvas insert edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

struct InsertCallPlan {
    callee: String,
    args: Vec<String>,
    fallible: bool,
    import: Option<TextEdit>,
}

fn insert_call_plan(src: &str, callee: &str, args: &[String]) -> InsertCallPlan {
    let Some(target) = core_target_for_callee(src, callee) else {
        return InsertCallPlan {
            callee: callee.to_string(),
            args: args.to_vec(),
            fallible: false,
            import: None,
        };
    };
    let module = normalize_core_module(&target.module, &target.member);
    let (prefix, import) = core_call_prefix_and_import(src, &module);
    let final_args = if args.is_empty() {
        core_default_args(&module, &target.member)
    } else {
        args.to_vec()
    };
    InsertCallPlan {
        callee: format!("{prefix}.{}", target.member),
        args: final_args,
        fallible: core_call_is_fallible(&module, &target.member),
        import,
    }
}

struct CoreCallTarget {
    module: String,
    member: String,
}

fn core_target_for_callee(src: &str, callee: &str) -> Option<CoreCallTarget> {
    let parts = callee.split('.').collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let member = parts.last()?.to_string();
    if parts.first() == Some(&"core") && parts.len() >= 3 {
        return Some(CoreCallTarget {
            module: parts[..parts.len() - 1].join("."),
            member,
        });
    }
    for import in core_imports(src) {
        if import.alias != parts[0] {
            continue;
        }
        let suffix = if parts.len() > 2 {
            format!(".{}", parts[1..parts.len() - 1].join("."))
        } else {
            String::new()
        };
        return Some(CoreCallTarget {
            module: format!("{}{}", import.module, suffix),
            member,
        });
    }
    None
}

fn normalize_core_module(module: &str, member: &str) -> String {
    if module == "core.encoding"
        && matches!(
            member,
            "parse" | "decode" | "decode_traced" | "to_string" | "to_string_pretty"
                | "canonical" | "events"
        )
    {
        return "core.encoding.json".to_string();
    }
    module.to_string()
}

fn core_call_prefix_and_import(src: &str, module: &str) -> (String, Option<TextEdit>) {
    if let Some(prefix) = imported_core_prefix(src, module) {
        return (prefix, None);
    }
    let alias = default_core_alias(module);
    let (offset, joins_use_block) = core_import_insert_point(src);
    let suffix = if joins_use_block { "\n" } else { "\n\n" };
    let import = edit(
        SourceSpan {
            start: offset,
            end: offset,
        },
        &format!("use {module} as {alias}{suffix}"),
    );
    (alias, Some(import))
}

fn imported_core_prefix(src: &str, module: &str) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for import in core_imports(src) {
        if import.module == module {
            return Some(import.alias);
        }
        let prefix = format!("{}.", import.module);
        if module.starts_with(&prefix) {
            let suffix = &module[prefix.len()..];
            let candidate = format!("{}.{}", import.alias, suffix);
            if best
                .as_ref()
                .map_or(true, |(len, _)| import.module.len() > *len)
            {
                best = Some((import.module.len(), candidate));
            }
        }
    }
    best.map(|(_, alias)| alias)
}

struct CoreImport {
    module: String,
    alias: String,
}

fn core_imports(src: &str) -> Vec<CoreImport> {
    let mut imports = Vec::new();
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or("").trim();
        let Some(rest) = code.strip_prefix("use core.") else {
            continue;
        };
        let full = format!("core.{rest}");
        let (module, alias) = if let Some((module, alias)) = full.split_once(" as ") {
            (module.trim(), ident_prefix(alias.trim()))
        } else {
            let module = full.trim();
            (module, default_core_alias(module))
        };
        if !module.is_empty() && !alias.is_empty() {
            imports.push(CoreImport {
                module: module.to_string(),
                alias,
            });
        }
    }
    imports
}

fn ident_prefix(text: &str) -> String {
    text.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

fn default_core_alias(module: &str) -> String {
    module.rsplit('.').next().unwrap_or("core").to_string()
}

fn core_import_insert_point(src: &str) -> (usize, bool) {
    let mut offset = 0usize;
    let mut insert_at = 0usize;
    let mut saw_use = false;
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("use ") {
            saw_use = true;
            insert_at = offset + line.len();
        } else if saw_use {
            break;
        } else if trimmed.is_empty() || trimmed.starts_with("//") {
            insert_at = offset + line.len();
        } else {
            break;
        }
        offset += line.len();
    }
    (insert_at, saw_use)
}

fn core_default_args(module: &str, member: &str) -> Vec<String> {
    core_member_params(module, member, "").into_iter()
        .map(|(_, ty)| default_arg_for_type(&ty))
        .collect()
}

fn core_call_is_fallible(module: &str, member: &str) -> bool {
    matches!(
        (module, member),
        (
            "core.encoding.json"
                | "core.encoding.jsonl"
                | "core.encoding.csv"
                | "core.encoding.toml"
                | "core.encoding.yaml"
                | "core.encoding.xml"
                | "core.encoding.cbor"
                | "core.encoding.hex"
                | "core.encoding.base64"
                | "core.encoding.base32",
            "parse" | "decode" | "decode_traced" | "decode_url"
        )
    )
}

pub(super) fn canvas_action_candidate(
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
        jet_driver::Formatter::format_source(&changed).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let tmp = temp_canvas_check_path(path);
    fs::write(&tmp, &formatted).map_err(|e| edit_error("io", &e.to_string()))?;
    let (check, _) = jet_driver::Driver::check_file(&tmp.display().to_string(), None, true);
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

pub(super) fn apply_insert_structural(
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
            if !anchor.fallible {
                return Err(edit_error("unavailable", "needs a fallible function"));
            }
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

pub(super) fn apply_create_comment_region(
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

pub(super) fn apply_update_comment_region(
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

pub(super) fn apply_delete_comment_region(path: &Path, src: &str, region_id: &str) -> Result<String, String> {
    apply_delete_hint_region(path, src, region_id, "comment")
}

pub(super) fn apply_delete_hint_region(
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

pub(super) fn apply_create_collapse_region(
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

pub(super) fn extract_inline_candidate(
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

pub(super) fn inline_helper_candidate(
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

#[derive(Clone)]
struct StatementLoc {
    anchor: SourceSpan,
    full: SourceSpan,
    block: Vec<usize>,
    index: usize,
}

pub(super) fn apply_reorder_statements(
    path: &Path,
    src: &str,
    graph_id: &str,
    moved: SourceSpan,
    anchor: SourceSpan,
    position: &str,
) -> Result<String, String> {
    let Some(name_span) = graph_id_name_span(graph_id) else {
        return Err(edit_error(
            "bad_request",
            "Canvas graph id is not a function graph",
        ));
    };
    let path_str = path.to_string_lossy();
    let (diags, bundle) = jet_driver::Driver::check_file(&path_str, None, true);
    let errors = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(diagnostics_error(path, src, &errors));
    }
    let Some(bundle) = bundle else {
        return Err(edit_error(
            "check",
            "Canvas could not read checked statement facts",
        ));
    };
    let Some(func) = find_func_by_name_span(&bundle, name_span) else {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    };
    let mut locs = Vec::new();
    collect_statement_locs(src, &func.body, &mut Vec::new(), &mut locs);
    let Some(moved_loc) = locs.iter().find(|loc| same_span(loc.anchor, moved)).cloned() else {
        return Err(edit_error(
            "not_found",
            "Canvas step to move no longer exists",
        ));
    };
    let Some(anchor_loc) = locs.iter().find(|loc| same_span(loc.anchor, anchor)).cloned() else {
        return Err(edit_error(
            "not_found",
            "Canvas anchor step no longer exists",
        ));
    };
    if moved_loc.block != anchor_loc.block {
        return Err(edit_error(
            "bad_request",
            "can't move a step into a different branch yet",
        ));
    }
    if moved_loc.index == anchor_loc.index {
        return Ok(edit_ok(false, path));
    }
    if moved_loc.full.start <= anchor_loc.full.start && anchor_loc.full.start < moved_loc.full.end {
        return Err(edit_error(
            "bad_request",
            "can't anchor a step inside the moved source yet",
        ));
    }
    let insert = match position {
        "before" => anchor_loc.full.start,
        "after" => anchor_loc.full.end,
        _ => {
            return Err(edit_error(
                "bad_request",
                "Canvas reorder position must be before or after",
            ))
        }
    };
    let chunk = src
        .get(moved_loc.full.start..moved_loc.full.end)
        .ok_or_else(|| edit_error("bad_request", "Canvas moved source span is stale"))?
        .to_string();
    let mut changed = String::with_capacity(src.len());
    changed.push_str(&src[..moved_loc.full.start]);
    changed.push_str(&src[moved_loc.full.end..]);
    let mut insert_at = insert;
    if insert_at > moved_loc.full.start {
        insert_at = insert_at.saturating_sub(moved_loc.full.end - moved_loc.full.start);
    }
    insert_at = insert_at.min(changed.len());
    changed.insert_str(insert_at, &chunk);
    write_checked_formatted(path, src, &changed)
}

pub(super) fn apply_add_pattern_arm(
    path: &Path,
    src: &str,
    graph_id: &str,
    node_span: SourceSpan,
    pattern: &str,
) -> Result<String, String> {
    let func = checked_func_for_graph(path, src, graph_id)?;
    let head = normalize_pattern_arm_head(pattern);
    if head.is_empty() {
        return Err(edit_error("bad_request", "pattern arm text is empty"));
    }
    let mut target = None;
    find_pattern_target(&func.body, node_span, &mut target);
    let Some(target) = target else {
        return Err(edit_error(
            "not_found",
            "Canvas pattern node no longer exists",
        ));
    };
    let body = fresh_arm_body(func);
    let changed = match target {
        PatternTarget::Branch(ifs) => add_arm_to_branch(src, ifs, &head, &body)?,
        PatternTarget::Switch { arms, else_body, span, .. } => {
            // Insert right after the last existing arm so the new arm never
            // lands inside the else body's block (its first-statement
            // offset sits *inside* `else -> { ... }`, not before it — that
            // splice used to tear a multi-line else block in half).
            let insert = if let Some(last) = arms.last() {
                line_after(src, last.span.end)
            } else if let Some(else_body) = else_body {
                line_start(src, first_stmt_start(else_body).unwrap_or(span.end))
            } else {
                dispatch_body_insert_offset(src, span)
            };
            let indent = indentation_at(src, insert);
            let arm = format!("{indent}{head} -> {{\n{indent}    {body}\n{indent}}}\n");
            FixEngine::apply_edits(
                src,
                &[edit(
                    SourceSpan {
                        start: insert,
                        end: insert,
                    },
                    &arm,
                )],
            )
            .map_err(|_| edit_error("overlap", "Canvas pattern arm insert overlapped"))?
        }
    };
    write_checked_formatted(path, src, &changed)
}

pub(super) fn apply_edit_pattern_arm(
    path: &Path,
    src: &str,
    graph_id: &str,
    pattern_span: SourceSpan,
    pattern: &str,
) -> Result<String, String> {
    let func = checked_func_for_graph(path, src, graph_id)?;
    let head = normalize_pattern_arm_head(pattern);
    if head.is_empty() {
        return Err(edit_error("bad_request", "pattern arm text is empty"));
    }
    if !pattern_span_belongs_to_graph(&func.body, pattern_span) {
        return Err(edit_error(
            "not_found",
            "Canvas pattern arm no longer exists",
        ));
    }
    let changed = FixEngine::apply_edits(src, &[edit(pattern_span, &head)])
        .map_err(|_| edit_error("overlap", "Canvas pattern arm edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

pub(super) fn apply_remove_pattern_arm(
    path: &Path,
    src: &str,
    graph_id: &str,
    pattern_span: SourceSpan,
) -> Result<String, String> {
    let func = checked_func_for_graph(path, src, graph_id)?;
    let mut arm = None;
    find_pattern_arm_remove_span(src, &func.body, pattern_span, &mut arm);
    let Some(info) = arm else {
        return Err(edit_error(
            "not_found",
            "Canvas pattern arm no longer exists",
        ));
    };
    if info.only_arm_without_else {
        return Err(edit_error(
            "bad_request",
            "can't remove the last pattern arm; the branch would have no path left",
        ));
    }
    let changed = FixEngine::apply_edits(src, &[edit(info.remove_span, "")])
        .map_err(|_| edit_error("overlap", "Canvas pattern arm delete overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

pub(super) fn apply_append_multi_input(
    path: &Path,
    src: &str,
    node_span: SourceSpan,
    element: &str,
) -> Result<String, String> {
    let mut target = None;
    let func = checked_func_containing_span(path, src, node_span)?;
    find_multi_input_target(&func.body, node_span, &mut target);
    let Some(target) = target else {
        return Err(edit_error(
            "not_found",
            "Canvas multi-input node no longer exists",
        ));
    };
    let (span, count) = match target {
        MultiInputTarget::List { span, count } | MultiInputTarget::FanOut { span, count } => {
            (span, count)
        }
    };
    let insert = src
        .get(span.start..span.end)
        .and_then(|chunk| chunk.rfind(']').map(|idx| span.start + idx))
        .ok_or_else(|| edit_error("bad_request", "Canvas multi-input source span is stale"))?;
    let prefix = if count == 0 { "" } else { ", " };
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: insert,
                end: insert,
            },
            &format!("{prefix}{element}"),
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas multi-input append overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

pub(super) fn apply_remove_multi_input_element(
    path: &Path,
    src: &str,
    node_span: SourceSpan,
    element_span: SourceSpan,
) -> Result<String, String> {
    let func = checked_func_containing_span(path, src, node_span)?;
    if !multi_input_element_belongs_to_graph(&func.body, node_span, element_span) {
        return Err(edit_error(
            "not_found",
            "Canvas multi-input element no longer exists",
        ));
    }
    let remove = comma_separated_remove_span(src, node_span, element_span)?;
    let changed = FixEngine::apply_edits(src, &[edit(remove, "")])
        .map_err(|_| edit_error("overlap", "Canvas multi-input delete overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn checked_func_for_graph(
    path: &Path,
    src: &str,
    graph_id: &str,
) -> Result<&'static AST::Func, String> {
    let Some(name_span) = graph_id_name_span(graph_id) else {
        return Err(edit_error(
            "bad_request",
            "Canvas graph id is not a function graph",
        ));
    };
    checked_func_by_name_span(path, src, name_span)
}

fn checked_func_containing_span(
    path: &Path,
    src: &str,
    span: SourceSpan,
) -> Result<&'static AST::Func, String> {
    let bundle = checked_bundle(path, src)?;
    let func = bundle
        .modules
        .iter()
        .find_map(|module| find_func_containing_span(&module.items, span));
    let Some(func) = func else {
        return Err(edit_error(
            "not_found",
            "Canvas source node no longer belongs to a function",
        ));
    };
    Ok(func)
}

fn checked_func_by_name_span(
    path: &Path,
    src: &str,
    name_span: SourceSpan,
) -> Result<&'static AST::Func, String> {
    let bundle = checked_bundle(path, src)?;
    let Some(func) = find_func_by_name_span(&bundle, name_span) else {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    };
    Ok(func)
}

fn checked_bundle(path: &Path, src: &str) -> Result<&'static AST::ProgramBundle, String> {
    let path_str = path.to_string_lossy();
    let (diags, bundle) = jet_driver::Driver::check_file(&path_str, None, true);
    let errors = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(diagnostics_error(path, src, &errors));
    }
    let Some(bundle) = bundle else {
        return Err(edit_error(
            "check",
            "Canvas could not read checked pattern facts",
        ));
    };
    Ok(Box::leak(Box::new(bundle)))
}

enum PatternTarget<'a> {
    Branch(&'a AST::IfStmt),
    Switch {
        arms: &'a [AST::SwitchArm],
        else_body: Option<&'a Vec<Stmt>>,
        span: SourceSpan,
    },
}

fn find_pattern_target<'a>(
    stmts: &'a [Stmt],
    node_span: SourceSpan,
    out: &mut Option<PatternTarget<'a>>,
) {
    if out.is_some() {
        return;
    }
    for stmt in stmts {
        match stmt {
            Stmt::If(ifs) => {
                if same_span(ifs.cond.span().into(), node_span)
                    && matches!(ifs.cond, Expr::PatternTest { .. })
                {
                    *out = Some(PatternTarget::Branch(ifs));
                    return;
                }
                find_pattern_target(&ifs.then_body, node_span, out);
                if let Some(branch) = &ifs.else_branch {
                    match branch {
                        AST::ElseBranch::Else(body) => find_pattern_target(body, node_span, out),
                        AST::ElseBranch::ElseIf(next) => {
                            find_pattern_target_in_if(next, node_span, out)
                        }
                    }
                }
            }
            Stmt::Switch {
                arms,
                else_body,
                span,
                ..
            }
            | Stmt::ComptimeSwitch {
                arms,
                else_body,
                span,
                ..
            } => {
                if same_span((*span).into(), node_span) {
                    *out = Some(PatternTarget::Switch {
                        arms,
                        else_body: else_body.as_ref(),
                        span: (*span).into(),
                    });
                    return;
                }
                for arm in arms {
                    find_pattern_target(&arm.body, node_span, out);
                }
                if let Some(body) = else_body {
                    find_pattern_target(body, node_span, out);
                }
            }
            _ => find_pattern_target_in_children(stmt, node_span, out),
        }
    }
}

fn find_pattern_target_in_if<'a>(
    ifs: &'a AST::IfStmt,
    node_span: SourceSpan,
    out: &mut Option<PatternTarget<'a>>,
) {
    if same_span(ifs.cond.span().into(), node_span) && matches!(ifs.cond, Expr::PatternTest { .. })
    {
        *out = Some(PatternTarget::Branch(ifs));
        return;
    }
    find_pattern_target(&ifs.then_body, node_span, out);
    if let Some(branch) = &ifs.else_branch {
        match branch {
            AST::ElseBranch::Else(body) => find_pattern_target(body, node_span, out),
            AST::ElseBranch::ElseIf(next) => find_pattern_target_in_if(next, node_span, out),
        }
    }
}

fn find_pattern_target_in_children<'a>(
    stmt: &'a Stmt,
    node_span: SourceSpan,
    out: &mut Option<PatternTarget<'a>>,
) {
    match stmt {
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_pattern_target(body, node_span, out),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            find_pattern_target(then_body, node_span, out);
            if let Some(body) = else_body {
                find_pattern_target(body, node_span, out);
            }
        }
        _ => {}
    }
}

struct RemoveArmInfo {
    remove_span: SourceSpan,
    only_arm_without_else: bool,
}

fn find_pattern_arm_remove_span(
    src: &str,
    stmts: &[Stmt],
    pattern_span: SourceSpan,
    out: &mut Option<RemoveArmInfo>,
) {
    if out.is_some() {
        return;
    }
    for stmt in stmts {
        match stmt {
            Stmt::If(ifs) => {
                if let Expr::PatternTest { pattern, .. } = &ifs.cond {
                    if same_span(pattern.span().into(), pattern_span) {
                        *out = Some(RemoveArmInfo {
                            remove_span: stmt_text_span(src, stmt),
                            only_arm_without_else: true,
                        });
                        return;
                    }
                }
                find_pattern_arm_remove_span(src, &ifs.then_body, pattern_span, out);
                if let Some(branch) = &ifs.else_branch {
                    match branch {
                        AST::ElseBranch::Else(body) => {
                            find_pattern_arm_remove_span(src, body, pattern_span, out)
                        }
                        AST::ElseBranch::ElseIf(next) => {
                            find_pattern_arm_remove_span_in_if(src, next, pattern_span, out)
                        }
                    }
                }
            }
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    if same_span(arm.cond.span().into(), pattern_span) {
                        *out = Some(RemoveArmInfo {
                            remove_span: stmt_arm_text_span(src, arm),
                            only_arm_without_else: arms.len() == 1 && else_body.is_none(),
                        });
                        return;
                    }
                    find_pattern_arm_remove_span(src, &arm.body, pattern_span, out);
                }
                if let Some(body) = else_body {
                    find_pattern_arm_remove_span(src, body, pattern_span, out);
                }
            }
            _ => find_pattern_arm_remove_span_in_children(src, stmt, pattern_span, out),
        }
    }
}

fn find_pattern_arm_remove_span_in_if(
    src: &str,
    ifs: &AST::IfStmt,
    pattern_span: SourceSpan,
    out: &mut Option<RemoveArmInfo>,
) {
    if let Expr::PatternTest { pattern, .. } = &ifs.cond {
        if same_span(pattern.span().into(), pattern_span) {
            *out = Some(RemoveArmInfo {
                remove_span: SourceSpan {
                    start: line_start(src, ifs.span.start),
                    end: line_after(src, ifs.span.end),
                },
                only_arm_without_else: true,
            });
            return;
        }
    }
    find_pattern_arm_remove_span(src, &ifs.then_body, pattern_span, out);
    if let Some(branch) = &ifs.else_branch {
        match branch {
            AST::ElseBranch::Else(body) => {
                find_pattern_arm_remove_span(src, body, pattern_span, out)
            }
            AST::ElseBranch::ElseIf(next) => {
                find_pattern_arm_remove_span_in_if(src, next, pattern_span, out)
            }
        }
    }
}

fn find_pattern_arm_remove_span_in_children(
    src: &str,
    stmt: &Stmt,
    pattern_span: SourceSpan,
    out: &mut Option<RemoveArmInfo>,
) {
    match stmt {
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => {
            find_pattern_arm_remove_span(src, body, pattern_span, out)
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            find_pattern_arm_remove_span(src, then_body, pattern_span, out);
            if let Some(body) = else_body {
                find_pattern_arm_remove_span(src, body, pattern_span, out);
            }
        }
        _ => {}
    }
}

fn pattern_span_belongs_to_graph(stmts: &[Stmt], pattern_span: SourceSpan) -> bool {
    let mut found = false;
    find_pattern_span(stmts, pattern_span, &mut found);
    found
}

fn find_pattern_span(stmts: &[Stmt], pattern_span: SourceSpan, found: &mut bool) {
    if *found {
        return;
    }
    for stmt in stmts {
        match stmt {
            Stmt::If(ifs) => {
                if let Expr::PatternTest { pattern, .. } = &ifs.cond {
                    if same_span(pattern.span().into(), pattern_span) {
                        *found = true;
                        return;
                    }
                }
                find_pattern_span(&ifs.then_body, pattern_span, found);
                if let Some(branch) = &ifs.else_branch {
                    match branch {
                        AST::ElseBranch::Else(body) => find_pattern_span(body, pattern_span, found),
                        AST::ElseBranch::ElseIf(next) => {
                            find_pattern_span_in_if(next, pattern_span, found)
                        }
                    }
                }
            }
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    if same_span(arm.cond.span().into(), pattern_span) {
                        *found = true;
                        return;
                    }
                    find_pattern_span(&arm.body, pattern_span, found);
                }
                if let Some(body) = else_body {
                    find_pattern_span(body, pattern_span, found);
                }
            }
            _ => find_pattern_span_in_children(stmt, pattern_span, found),
        }
    }
}

fn find_pattern_span_in_if(ifs: &AST::IfStmt, pattern_span: SourceSpan, found: &mut bool) {
    if let Expr::PatternTest { pattern, .. } = &ifs.cond {
        if same_span(pattern.span().into(), pattern_span) {
            *found = true;
            return;
        }
    }
    find_pattern_span(&ifs.then_body, pattern_span, found);
    if let Some(branch) = &ifs.else_branch {
        match branch {
            AST::ElseBranch::Else(body) => find_pattern_span(body, pattern_span, found),
            AST::ElseBranch::ElseIf(next) => find_pattern_span_in_if(next, pattern_span, found),
        }
    }
}

fn find_pattern_span_in_children(stmt: &Stmt, pattern_span: SourceSpan, found: &mut bool) {
    match stmt {
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_pattern_span(body, pattern_span, found),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            find_pattern_span(then_body, pattern_span, found);
            if let Some(body) = else_body {
                find_pattern_span(body, pattern_span, found);
            }
        }
        _ => {}
    }
}

enum MultiInputTarget {
    List { span: SourceSpan, count: usize },
    FanOut { span: SourceSpan, count: usize },
}

fn find_multi_input_target(
    stmts: &[Stmt],
    node_span: SourceSpan,
    out: &mut Option<MultiInputTarget>,
) {
    if out.is_some() {
        return;
    }
    for stmt in stmts {
        find_multi_input_in_stmt(stmt, node_span, out);
    }
}

fn find_multi_input_in_stmt(stmt: &Stmt, node_span: SourceSpan, out: &mut Option<MultiInputTarget>) {
    if out.is_some() {
        return;
    }
    match stmt {
        Stmt::Val(b) => find_multi_input_in_expr(&b.init, node_span, out),
        Stmt::Assign { target, value, .. } => {
            find_multi_input_in_lvalue(target, node_span, out);
            find_multi_input_in_expr(value, node_span, out);
        }
        Stmt::Expr(e) => find_multi_input_in_expr(e, node_span, out),
        Stmt::Return(Some(e), _) => find_multi_input_in_expr(e, node_span, out),
        Stmt::Yield(e, _) => find_multi_input_in_expr(e, node_span, out),
        Stmt::If(ifs) => {
            find_multi_input_in_expr(&ifs.cond, node_span, out);
            find_multi_input_target(&ifs.then_body, node_span, out);
            if let Some(branch) = &ifs.else_branch {
                match branch {
                    AST::ElseBranch::Else(body) => find_multi_input_target(body, node_span, out),
                    AST::ElseBranch::ElseIf(next) => {
                        find_multi_input_in_stmt(&Stmt::If((**next).clone()), node_span, out)
                    }
                }
            }
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            find_multi_input_in_expr(subject, node_span, out);
            for arm in arms {
                find_multi_input_in_expr(&arm.cond, node_span, out);
                find_multi_input_target(&arm.body, node_span, out);
            }
            if let Some(body) = else_body {
                find_multi_input_target(body, node_span, out);
            }
        }
        Stmt::While { cond, body, .. } => {
            find_multi_input_in_expr(cond, node_span, out);
            find_multi_input_target(body, node_span, out);
        }
        Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_multi_input_target(body, node_span, out),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            find_multi_input_target(then_body, node_span, out);
            if let Some(body) = else_body {
                find_multi_input_target(body, node_span, out);
            }
        }
        Stmt::Return(None, _)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(_, _)
        | Stmt::ContinueLabel(_, _) => {}
    }
}

fn find_multi_input_in_lvalue(
    target: &AST::LValue,
    node_span: SourceSpan,
    out: &mut Option<MultiInputTarget>,
) {
    match target {
        AST::LValue::Index { base, index, .. } => {
            find_multi_input_in_expr(base, node_span, out);
            find_multi_input_in_expr(index, node_span, out);
        }
        AST::LValue::Field { base, .. } => find_multi_input_in_expr(base, node_span, out),
        _ => {}
    }
}

fn find_multi_input_in_expr(
    expr: &Expr,
    node_span: SourceSpan,
    out: &mut Option<MultiInputTarget>,
) {
    if out.is_some() {
        return;
    }
    match expr {
        Expr::ListLit(items, span) if same_span((*span).into(), node_span) => {
            *out = Some(MultiInputTarget::List {
                span: (*span).into(),
                count: items.len(),
            });
        }
        Expr::FanOut { items, span, .. } if same_span((*span).into(), node_span) => {
            *out = Some(MultiInputTarget::FanOut {
                span: (*span).into(),
                count: items.len(),
            });
        }
        _ => walk_expr_children_for_multi_input(expr, node_span, out),
    }
}

fn walk_expr_children_for_multi_input(
    expr: &Expr,
    node_span: SourceSpan,
    out: &mut Option<MultiInputTarget>,
) {
    match expr {
        Expr::Call(c) => {
            for arg in &c.args {
                find_multi_input_in_expr(&arg.expr, node_span, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            find_multi_input_in_expr(receiver, node_span, out);
            for arg in args {
                find_multi_input_in_expr(&arg.expr, node_span, out);
            }
        }
        Expr::FanOut { callee, items, .. } => {
            find_multi_input_in_expr(callee, node_span, out);
            for item in items {
                find_multi_input_in_expr(item, node_span, out);
            }
        }
        Expr::ListLit(items, _) => {
            for item in items {
                find_multi_input_in_expr(item, node_span, out);
            }
        }
        Expr::Index { base, index, .. } => {
            find_multi_input_in_expr(base, node_span, out);
            find_multi_input_in_expr(index, node_span, out);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            find_multi_input_in_expr(base, node_span, out);
            find_multi_input_in_expr(start, node_span, out);
            find_multi_input_in_expr(end, node_span, out);
        }
        Expr::MapLit(items, _) => {
            for (key, value) in items {
                find_multi_input_in_expr(key, node_span, out);
                find_multi_input_in_expr(value, node_span, out);
            }
        }
        Expr::Binary(_, left, right, _) => {
            find_multi_input_in_expr(left, node_span, out);
            find_multi_input_in_expr(right, node_span, out);
        }
        Expr::Unary(_, expr, _)
        | Expr::Try(expr, ..)
        | Expr::Present(expr, _)
        | Expr::Ok(expr, _)
        | Expr::Err(expr, _)
        | Expr::Paren(expr, _)
        | Expr::Copy(expr, _)
        | Expr::Field(expr, _, _)
        | Expr::RawOf(expr, _)
        | Expr::Tainted(expr, _, _) => find_multi_input_in_expr(expr, node_span, out),
        Expr::PatternTest { subject, .. } => find_multi_input_in_expr(subject, node_span, out),
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            find_multi_input_in_expr(cond, node_span, out);
            find_multi_input_target(then_body, node_span, out);
            find_multi_input_in_expr(then_value, node_span, out);
            find_multi_input_target(else_body, node_span, out);
            find_multi_input_in_expr(else_value, node_span, out);
        }
        Expr::TupleLit(items, _, _) => {
            for (_, item) in items {
                find_multi_input_in_expr(item, node_span, out);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                find_multi_input_in_expr(value, node_span, out);
            }
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    AST::EnumLitArg::Positional(expr) => {
                        find_multi_input_in_expr(expr, node_span, out)
                    }
                    AST::EnumLitArg::Named { expr, .. } => {
                        find_multi_input_in_expr(expr, node_span, out)
                    }
                }
            }
        }
        _ => {}
    }
}

fn multi_input_element_belongs_to_graph(
    stmts: &[Stmt],
    node_span: SourceSpan,
    element_span: SourceSpan,
) -> bool {
    let mut found = false;
    find_multi_input_element(stmts, node_span, element_span, &mut found);
    found
}

fn find_multi_input_element(
    stmts: &[Stmt],
    node_span: SourceSpan,
    element_span: SourceSpan,
    found: &mut bool,
) {
    if *found {
        return;
    }
    let mut target = None;
    find_multi_input_target(stmts, node_span, &mut target);
    let _ = target;
    for stmt in stmts {
        find_multi_input_element_in_stmt(stmt, node_span, element_span, found);
    }
}

fn find_multi_input_element_in_stmt(
    stmt: &Stmt,
    node_span: SourceSpan,
    element_span: SourceSpan,
    found: &mut bool,
) {
    match stmt {
        Stmt::Val(b) => find_multi_input_element_in_expr(&b.init, node_span, element_span, found),
        Stmt::Expr(e) | Stmt::Return(Some(e), _) | Stmt::Yield(e, _) => {
            find_multi_input_element_in_expr(e, node_span, element_span, found)
        }
        Stmt::Assign { value, .. } => {
            find_multi_input_element_in_expr(value, node_span, element_span, found)
        }
        Stmt::If(ifs) => {
            find_multi_input_element_in_expr(&ifs.cond, node_span, element_span, found);
            find_multi_input_element(&ifs.then_body, node_span, element_span, found);
            if let Some(AST::ElseBranch::Else(body)) = &ifs.else_branch {
                find_multi_input_element(body, node_span, element_span, found);
            }
        }
        _ => {}
    }
}

fn find_multi_input_element_in_expr(
    expr: &Expr,
    node_span: SourceSpan,
    element_span: SourceSpan,
    found: &mut bool,
) {
    if *found {
        return;
    }
    match expr {
        Expr::ListLit(items, span) | Expr::FanOut { items, span, .. }
            if same_span((*span).into(), node_span) =>
        {
            *found = items
                .iter()
                .any(|item| same_span(item.span().into(), element_span));
        }
        _ => {
            let mut nested = None;
            walk_expr_children_for_multi_input(expr, node_span, &mut nested);
            if nested.is_some() {
                *found = true;
            }
        }
    }
}

fn normalize_pattern_arm_head(pattern: &str) -> String {
    let trimmed = pattern.trim();
    trimmed
        .strip_prefix("==")
        .map(str::trim)
        .unwrap_or(trimmed)
        .trim_end_matches("->")
        .trim()
        .to_string()
}

fn fresh_arm_body(func: &AST::Func) -> String {
    match func.return_type.as_ref().map(AST::Type::name) {
        Some(ret) if ret != "Void" => format!("return {}", default_arg_for_type(&ret)),
        _ => "print(\"canvas arm\")".to_string(),
    }
}

fn add_arm_to_branch(
    src: &str,
    ifs: &AST::IfStmt,
    new_head: &str,
    fresh_body: &str,
) -> Result<String, String> {
    let Expr::PatternTest {
        subject, pattern, ..
    } = &ifs.cond
    else {
        return Err(edit_error(
            "bad_request",
            "Canvas branch is not a pattern branch",
        ));
    };
    let subject_src = snippet(src, subject.span());
    let head = snippet(src, pattern.span());
    let then_body = block_body_source(src, &ifs.then_body, 2);
    let else_body = if let Some(AST::ElseBranch::Else(body)) = &ifs.else_branch {
        let body = block_body_source(src, body, 2);
        format!("    else -> {{\n{body}    }}\n")
    } else {
        String::new()
    };
    let replacement = format!(
        "if {subject_src} == {{\n    {head} -> {{\n{then_body}    }}\n    {new_head} -> {{\n        {fresh_body}\n    }}\n{else_body}}}"
    );
    FixEngine::apply_edits(src, &[edit(ifs.span.into(), &replacement)])
        .map_err(|_| edit_error("overlap", "Canvas pattern branch conversion overlapped"))
}

fn block_body_source(src: &str, stmts: &[Stmt], levels: usize) -> String {
    if stmts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let indent = "    ".repeat(levels);
    for stmt in stmts {
        let span = stmt_text_span(src, stmt);
        let text = src
            .get(span.start..span.end)
            .unwrap_or("")
            .trim_matches('\n')
            .trim();
        for line in text.lines() {
            out.push_str(&indent);
            out.push_str(line.trim());
            out.push('\n');
        }
    }
    out
}

fn first_stmt_start(stmts: &[Stmt]) -> Option<usize> {
    stmts.first().map(|stmt| stmt.span().start)
}

fn dispatch_body_insert_offset(src: &str, span: SourceSpan) -> usize {
    src.get(span.start..span.end)
        .and_then(|chunk| chunk.find('{').map(|idx| line_after(src, span.start + idx)))
        .unwrap_or(span.end)
}

fn stmt_arm_text_span(src: &str, arm: &AST::SwitchArm) -> SourceSpan {
    SourceSpan {
        start: line_start(src, arm.span.start),
        end: line_after(src, arm.span.end),
    }
}

fn comma_separated_remove_span(
    src: &str,
    container: SourceSpan,
    element: SourceSpan,
) -> Result<SourceSpan, String> {
    let Some(container_text) = src.get(container.start..container.end) else {
        return Err(edit_error("bad_request", "Canvas multi-input span is stale"));
    };
    let rel_start = element.start.saturating_sub(container.start);
    let rel_end = element.end.saturating_sub(container.start);
    let before = &container_text[..rel_start.min(container_text.len())];
    let after = &container_text[rel_end.min(container_text.len())..];
    if let Some(pos) = after.find(',') {
        let end = element.end + pos + 1;
        return Ok(SourceSpan {
            start: element.start,
            end: consume_trailing_space(src, end, container.end),
        });
    }
    if let Some(pos) = before.rfind(',') {
        let start = container.start + consume_leading_space_back(before, pos);
        return Ok(SourceSpan {
            start,
            end: element.end,
        });
    }
    Ok(element)
}

fn consume_trailing_space(src: &str, mut offset: usize, limit: usize) -> usize {
    while offset < limit && src.as_bytes().get(offset).is_some_and(u8::is_ascii_whitespace) {
        offset += 1;
    }
    offset
}

fn consume_leading_space_back(before: &str, comma: usize) -> usize {
    let mut start = comma;
    while start > 0 && before.as_bytes()[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    start
}

fn find_func_containing_span<'a>(items: &'a [Item], span: SourceSpan) -> Option<&'a AST::Func> {
    for item in items {
        match item {
            Item::Func(f) if span_within(span, func_source_span(f)) => return Some(f),
            Item::Struct(s) => {
                for method in &s.methods {
                    if span_within(span, func_source_span(method)) {
                        return Some(method);
                    }
                }
            }
            Item::Impl(i) => {
                for method in &i.methods {
                    if span_within(span, func_source_span(method)) {
                        return Some(method);
                    }
                }
            }
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    if let Some(found) = find_func_containing_span(body, span) {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn span_within(inner: SourceSpan, outer: SourceSpan) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

fn find_func_by_name_span<'a>(
    bundle: &'a AST::ProgramBundle,
    name_span: SourceSpan,
) -> Option<&'a AST::Func> {
    fn find_in_items<'a>(items: &'a [Item], name_span: SourceSpan) -> Option<&'a AST::Func> {
        for item in items {
            match item {
                Item::Func(f) if same_span(f.name_span.into(), name_span) => return Some(f),
                Item::Struct(s) => {
                    for method in &s.methods {
                        if same_span(method.name_span.into(), name_span) {
                            return Some(method);
                        }
                    }
                }
                Item::Impl(i) => {
                    for method in &i.methods {
                        if same_span(method.name_span.into(), name_span) {
                            return Some(method);
                        }
                    }
                }
                Item::CodeModule(m) => {
                    if let Some(body) = &m.body {
                        if let Some(found) = find_in_items(body, name_span) {
                            return Some(found);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    for module in &bundle.modules {
        if let Some(found) = find_in_items(&module.items, name_span) {
            return Some(found);
        }
    }
    None
}

fn collect_statement_locs(
    src: &str,
    stmts: &[Stmt],
    block: &mut Vec<usize>,
    out: &mut Vec<StatementLoc>,
) {
    for (index, stmt) in stmts.iter().enumerate() {
        out.push(StatementLoc {
            anchor: stmt_canvas_anchor(stmt).into(),
            full: stmt_text_span(src, stmt),
            block: block.clone(),
            index,
        });
        block.push(index);
        collect_child_statement_locs(src, stmt, block, out);
        block.pop();
    }
}

fn collect_child_statement_locs(
    src: &str,
    stmt: &Stmt,
    block: &mut Vec<usize>,
    out: &mut Vec<StatementLoc>,
) {
    match stmt {
        Stmt::If(ifs) => {
            block.push(0);
            collect_statement_locs(src, &ifs.then_body, block, out);
            block.pop();
            match &ifs.else_branch {
                Some(AST::ElseBranch::Else(body)) => {
                    block.push(1);
                    collect_statement_locs(src, body, block, out);
                    block.pop();
                }
                Some(AST::ElseBranch::ElseIf(next)) => {
                    block.push(1);
                    collect_child_statement_locs(src, &Stmt::If((**next).clone()), block, out);
                    block.pop();
                }
                None => {}
            }
        }
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => {
            block.push(0);
            collect_statement_locs(src, body, block, out);
            block.pop();
        }
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            for (i, arm) in arms.iter().enumerate() {
                block.push(i);
                collect_statement_locs(src, &arm.body, block, out);
                block.pop();
            }
            if let Some(body) = else_body {
                block.push(arms.len());
                collect_statement_locs(src, body, block, out);
                block.pop();
            }
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            block.push(0);
            collect_statement_locs(src, then_body, block, out);
            block.pop();
            if let Some(body) = else_body {
                block.push(1);
                collect_statement_locs(src, body, block, out);
                block.pop();
            }
        }
        _ => {}
    }
}

fn stmt_canvas_anchor(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::If(ifs) => ifs.cond.span(),
        Stmt::Val(b) => b.name_span,
        Stmt::Assign { target, .. } => target.span(),
        Stmt::Expr(e) => e.span(),
        Stmt::Return(_, span)
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::BreakLabel(_, span)
        | Stmt::ContinueLabel(_, span) => *span,
        _ => stmt.span(),
    }
}

fn stmt_text_span(src: &str, stmt: &Stmt) -> SourceSpan {
    let span = match stmt {
        Stmt::Val(b) => SourceSpan {
            start: b.name_span.start,
            end: b.init.span().end,
        },
        Stmt::Assign { target, value, .. } => SourceSpan {
            start: target.span().start,
            end: value.span().end,
        },
        Stmt::Expr(e) => e.span().into(),
        Stmt::Return(Some(e), span) => SourceSpan {
            start: span.start,
            end: e.span().end,
        },
        Stmt::If(ifs) => ifs.span.into(),
        _ => stmt.span().into(),
    };
    SourceSpan {
        start: line_start(src, span.start),
        end: line_after(src, span.end),
    }
}

fn same_span(a: SourceSpan, b: SourceSpan) -> bool {
    a.start == b.start && a.end == b.end
}

pub(super) fn write_checked_formatted(path: &Path, before: &str, candidate: &str) -> Result<String, String> {
    let formatted = jet_driver::Formatter::format_source(candidate)
        .map_err(|diags| diagnostics_error(path, candidate, &diags))?;
    let path_str = path.to_string_lossy();
    let abs = canonical_path(path);
    let (diags, _, _) =
        jet_driver::Driver::check_file_with_effect_facts(&path_str, Some((&abs, &formatted)), true);
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
