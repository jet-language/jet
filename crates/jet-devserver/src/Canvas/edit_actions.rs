use std::fs;
use std::path::{Path, PathBuf};

use jet_driver::Diagnostics::{Diagnostic, Severity, Span, TextEdit};
use jet_driver::FixEngine;
use jet_driver::AST::{self, Expr, Item, Stmt};
use jet_semindex::SourceSpan;

use super::debug_source_git::canonical_path;
use super::graph_helpers::{
    diagnostics_error, edit, edit_error, edit_ok, edit_ok_source, function_signature_span,
    graph_id_name_span, indentation_at, line_after, line_start, snippet,
    span_through_closing_parens,
};
use super::graph_json::{canvas_collapse_hints, func_source_span};
use super::graph_projection::trait_method_signature;
use super::project_scan::project_file;
use super::query_actions::default_arg_for_type;
use super::source_model::{write_source_if_unchanged, SourceWriteError};
use super::validation_json::{
    extract_params, find_comment_hint, find_hint_region, find_simple_helper, json_str,
    normalize_bounds, parse_simple_call, quoted_attr, replace_ident, validate_comment_alpha,
    validate_comment_color, validate_ident, wire_span_from_json_chunk,
};

pub(super) fn apply_noop(path: &Path, _src: &str) -> Result<String, String> {
    Ok(edit_ok(false, path))
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
    let formatted = jet_driver::Formatter::format_source(&changed)
        .map_err(|diags| diagnostics_error(path, &changed, &diags))?;
    let result = write_checked_source(path, src, &formatted)?;
    if formatted != src {
        record_canvas_rename(path, src, &formatted, from, to, &idx)?;
    }
    Ok(result)
}

fn record_canvas_rename(
    path: &Path,
    before: &str,
    after: &str,
    from: &str,
    to: &str,
    index: &jet_semindex::SemIndex,
) -> Result<(), String> {
    let project = path.parent().unwrap_or_else(|| Path::new("."));
    let directory = project.join(".jet/codemods");
    fs::create_dir_all(&directory)
        .map_err(|error| edit_error("io", &format!("could not record semantic rename: {error}")))?;
    let before_hash = jet_driver::SHA256::sha256_hex(before.as_bytes());
    let after_hash = jet_driver::SHA256::sha256_hex(after.as_bytes());
    let targets = index
        .definition_facts()
        .iter()
        .filter(|fact| fact.name == from)
        .map(|fact| {
            format!(
                "{{\"stable_id\":{},\"before\":{},\"after\":{},\"kind\":{},\"module_path\":{}}}",
                json_str(&fact.stable_id),
                json_str(&fact.human_identity),
                json_str(&format!("{}::{to}", fact.module_path)),
                json_str(&fact.kind),
                json_str(&fact.module_path),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let file = format!(
        "{{\"path\":{},\"before_hash\":{},\"after_hash\":{}}}",
        json_str(&path.display().to_string()),
        json_str(&before_hash),
        json_str(&after_hash),
    );
    let op = format!(
        "{{\"kind\":\"rename\",\"from\":{},\"to\":{},\"targets\":[{}],\"files\":[{}]}}",
        json_str(from),
        json_str(to),
        targets,
        file,
    );
    let base = format!("CanvasRename-{}", &after_hash[..12]);
    let mut receipt = directory.join(format!("{base}.log.json"));
    let mut suffix = 0;
    while receipt.exists() {
        suffix += 1;
        receipt = directory.join(format!("{base}-{suffix}.log.json"));
    }
    let log = format!(
        "{{\"schema\":2,\"name\":{},\"project\":{},\"semantic_ops\":[{}],\"files\":[{}]}}\n",
        json_str(&base),
        json_str(&project.display().to_string()),
        op,
        file,
    );
    fs::write(receipt, log)
        .map_err(|error| edit_error("io", &format!("could not record semantic rename: {error}")))
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
        format!(" {ret_type} ->")
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
    let Some(location) = find_trait_location(&bundle, path, trait_name) else {
        return Err(edit_error(
            "not_found",
            "Canvas trait was not found in source",
        ));
    };
    let trait_def = location.trait_def;
    if !trait_def.assoc_types.is_empty() {
        return Err(edit_error(
            "needs_associated_types",
            "Canvas will not guess associated type implementations; add each associated type in source first",
        ));
    }
    let mut body = String::new();
    for method in trait_def
        .methods
        .iter()
        .filter(|method| method.default_body.is_none())
    {
        let sig = trait_method_signature(src, method);
        body.push_str("    ");
        body.push_str(&sig);
        if method
            .return_type
            .as_ref()
            .is_some_and(|ret| ret.name() != "Void")
            && method.declared_effects.is_none()
            && !method.is_pure
        {
            body.push_str(" ->");
        }
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
    let local_trait_name = trait_name.rsplit('.').next().unwrap_or(trait_name);
    let insert_at = if location.scope.is_empty() {
        src.len()
    } else {
        line_start(src, location.module_close)
    };
    let item_indent = if location.scope.is_empty() {
        String::new()
    } else {
        indentation_at(src, trait_def.span.start)
    };
    let prefix = if insert_at == 0 || src[..insert_at].ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let method_body = body
        .lines()
        .map(|line| format!("{item_indent}{line}\n"))
        .collect::<String>();
    let impl_block = format!(
        "{prefix}{item_indent}impl {type_name}.{local_trait_name} {{\n{method_body}{item_indent}}}\n"
    );
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: insert_at,
                end: insert_at,
            },
            &impl_block,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas trait impl edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

struct TraitLocation<'a> {
    trait_def: &'a AST::TraitDef,
    scope: Vec<String>,
    module_close: usize,
}

fn source_trait_path(
    bundle: &AST::ProgramBundle,
    module_idx: usize,
    module_alias: &str,
    name: &str,
    span: Span,
    fallback_scope: &[String],
) -> Option<String> {
    let ledger = &bundle.name_ledger;
    let path = [
        format!("{module_alias}.{name}"),
        if fallback_scope.is_empty() {
            format!("{module_alias}.{name}")
        } else {
            format!("{module_alias}.{}.{}", fallback_scope.join("."), name)
        },
    ]
    .into_iter()
    .find_map(|candidate| ledger.display_path(module_idx, &candidate, Some(module_idx)))
    .or_else(|| ledger.canonical_path_at(module_idx, span.start, span.end))?;
    Some(
        path.strip_prefix(module_alias)
            .and_then(|rest| rest.strip_prefix('.'))
            .unwrap_or(&path)
            .to_string(),
    )
}

fn enclosing_inline_module(items: &[Item], position: usize) -> Option<(Vec<String>, usize)> {
    for item in items {
        let Item::CodeModule(module) = item else {
            continue;
        };
        let Some(body) = &module.body else {
            continue;
        };
        if !(module.span.start <= position && position < module.span.end) {
            continue;
        }
        if let Some((mut scope, close)) = enclosing_inline_module(body, position) {
            scope.insert(0, module.name.clone());
            return Some((scope, close));
        }
        return Some((vec![module.name.clone()], module.span.end.saturating_sub(1)));
    }
    None
}

fn find_trait_location<'a>(
    bundle: &'a AST::ProgramBundle,
    path: &Path,
    trait_name: &str,
) -> Option<TraitLocation<'a>> {
    let parts = trait_name.split('.').collect::<Vec<_>>();
    let local_name = parts.last().copied().unwrap_or(trait_name);
    let requested_scope = &parts[..parts.len().saturating_sub(1)];
    let source_path = canonical_path(path);

    fn find_in_items<'a>(
        items: &'a [Item],
        scope: &mut Vec<String>,
        requested_scope: &[&str],
        requested_name: &str,
        local_name: &str,
        module_idx: usize,
        module_alias: &str,
        root_items: &'a [Item],
        source_len: usize,
        bundle: &'a AST::ProgramBundle,
    ) -> Option<TraitLocation<'a>> {
        for item in items {
            match item {
                Item::Trait(t) => {
                    let display_path = source_trait_path(
                        bundle,
                        module_idx,
                        module_alias,
                        &t.name,
                        t.name_span,
                        scope,
                    );
                    let fallback_match = t.name == local_name
                        && scope
                            .iter()
                            .map(String::as_str)
                            .eq(requested_scope.iter().copied());
                    if display_path.as_deref() != Some(requested_name) && !fallback_match {
                        continue;
                    }
                    let (scope, module_close) =
                        enclosing_inline_module(root_items, t.name_span.start)
                            .unwrap_or_else(|| (scope.clone(), source_len));
                    let scope = display_path
                        .as_deref()
                        .and_then(|path| path.rsplit_once('.'))
                        .map(|(scope, _)| scope.split('.').map(str::to_string).collect())
                        .unwrap_or(scope);
                    return Some(TraitLocation {
                        trait_def: t,
                        scope,
                        module_close,
                    });
                }
                Item::CodeModule(m) => {
                    if let Some(body) = &m.body {
                        scope.push(m.name.clone());
                        if let Some(t) = find_in_items(
                            body,
                            scope,
                            requested_scope,
                            requested_name,
                            local_name,
                            module_idx,
                            module_alias,
                            root_items,
                            source_len,
                            bundle,
                        ) {
                            return Some(t);
                        }
                        scope.pop();
                    }
                }
                _ => {}
            }
        }
        None
    }
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        if canonical_path(&module.path) != source_path {
            continue;
        }
        if let Some(t) = find_in_items(
            &module.items,
            &mut Vec::new(),
            requested_scope,
            trait_name,
            local_name,
            module_idx,
            &module.alias,
            &module.items,
            module.source.len(),
            bundle,
        ) {
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
    wire_origin_pin_id: Option<&str>,
    wire_target_pin: Option<&str>,
    module_path: Option<&str>,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let final_args = if args.is_empty() {
        wire_expr
            .map(|expr| vec![expr.to_string()])
            .unwrap_or_default()
    } else {
        args.to_vec()
    };
    let plan = insert_call_plan(src, callee, &final_args, module_path);
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
    let (insert_offset, indent) = call_insert_target(
        src,
        graph_id,
        &projection,
        anchor,
        wire_origin_pin_id,
        wire_target_pin,
    )?;
    let stmt = match bind {
        Some(name) => format!("{indent}{name} :: {call}\n"),
        None => format!("{indent}{call}\n"),
    };
    let mut edits = vec![edit(
        SourceSpan {
            start: insert_offset,
            end: insert_offset,
        },
        &stmt,
    )];
    if let Some(import) = plan.import {
        edits.push(import);
    }
    let changed = FixEngine::apply_edits(src, &edits)
        .map_err(|_| edit_error("overlap", "Canvas insert edit overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn call_insert_target(
    src: &str,
    graph_id: &str,
    projection: &super::schema_api::Projection,
    anchor: &super::schema_api::GraphEditAnchor,
    wire_origin_pin_id: Option<&str>,
    _wire_target_pin: Option<&str>,
) -> Result<(usize, String), String> {
    let Some(pin_id) = wire_origin_pin_id else {
        return Ok((anchor.insert_offset, "    ".to_string()));
    };
    let (node_id, direction) = pin_id
        .rsplit_once(":input:")
        .map(|(node, _)| (node, "input"))
        .or_else(|| {
            pin_id
                .rsplit_once(":output:")
                .map(|(node, _)| (node, "output"))
        })
        .ok_or_else(|| edit_error("not_found", "Canvas insertion pin no longer exists"))?;
    let node = projection
        .node_refs
        .iter()
        .filter(|node| node.graph_id == graph_id)
        .filter(|node| node.node_id == node_id || node.node_id.ends_with(&format!(":{node_id}")))
        .max_by_key(|node| node.span.end.saturating_sub(node.span.start))
        .ok_or_else(|| edit_error("not_found", "Canvas insertion pin no longer exists"))?;
    if node.node_id.ends_with(":entry") {
        return Ok((anchor.insert_offset, "    ".to_string()));
    }
    let indent = indentation_at(src, node.span.start);
    let statement_indent = if indent.is_empty() {
        "    ".to_string()
    } else {
        indent
    };
    let line = line_start(src, node.span.start);
    let node_end = node.span.end.min(src.len());
    let line_end = src[node_end..]
        .find('\n')
        .map(|offset| node_end + offset)
        .unwrap_or(src.len());
    let inline_body = line == line_start(src, anchor.insert_offset)
        && src
            .get(line..node.span.start.min(src.len()))
            .is_some_and(|prefix| prefix.contains('{'))
        && src
            .get(node_end..line_end)
            .is_some_and(|suffix| suffix.contains('}'));
    if inline_body {
        let insert = if direction == "input" {
            anchor.insert_offset
        } else {
            src[node_end..line_end]
                .find('}')
                .map(|offset| node_end + offset)
                .unwrap_or(line_end)
        };
        return Ok((insert, format!("\n{statement_indent}")));
    }
    let insert = if direction == "input" {
        line
    } else {
        line_after(src, node.span.end)
    };
    Ok((insert, statement_indent))
}

struct InsertCallPlan {
    callee: String,
    args: Vec<String>,
    fallible: bool,
    import: Option<TextEdit>,
}

fn insert_call_plan(
    src: &str,
    callee: &str,
    args: &[String],
    module_path: Option<&str>,
) -> InsertCallPlan {
    let Some(target) = core_target_for_callee(src, callee, module_path) else {
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

fn core_target_for_callee(
    src: &str,
    callee: &str,
    module_path: Option<&str>,
) -> Option<CoreCallTarget> {
    let parts = callee.split('.').collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let member = parts.last()?.to_string();
    if let Some(module) = module_path {
        let alias = default_core_alias(module);
        if module.starts_with("core.")
            && parts.len() == 2
            && parts.first().copied() == Some(alias.as_str())
        {
            return Some(CoreCallTarget {
                module: module.to_string(),
                member,
            });
        }
    }
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
    let alias = parts[0];
    if let Some(module) = jet_driver::Syntax::core_module_for_alias(alias) {
        let suffix = if parts.len() > 2 {
            format!(".{}", parts[1..parts.len() - 1].join("."))
        } else {
            String::new()
        };
        return Some(CoreCallTarget {
            module: format!("{module}{suffix}"),
            member,
        });
    }
    None
}

fn normalize_core_module(module: &str, member: &str) -> String {
    jet_driver::Syntax::core_module_for_call(module, member)
        .unwrap_or(module)
        .to_string()
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
    core_call_signature(module, member)
        .map(|(params, _)| {
            params
                .into_iter()
                .map(|(_, ty)| default_arg_for_type(&ty.name()))
                .collect()
        })
        .unwrap_or_default()
}

fn core_call_signature(
    module: &str,
    member: &str,
) -> Option<(Vec<(AST::AccessConvention, AST::Type)>, Option<AST::Type>)> {
    if let Some(row) = jet_driver::Syntax::core_call(module, member) {
        if let Some(signature) = jet_driver::Sema::core_fixed_sig_for_row(row) {
            return Some(signature);
        }
    }
    jet_driver::Sema::core_call_surface_signature(module, member)
}

fn core_call_is_fallible(module: &str, member: &str) -> bool {
    if let Some(row) = jet_driver::Syntax::core_call(module, member) {
        if let Some((_, ret)) = jet_driver::Sema::core_fixed_sig_for_row(row) {
            return ret.is_some_and(|ret| ret.is_fallible());
        }
    }
    jet_driver::Sema::core_call_is_fallible(module, member)
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
    let Some((_, descriptor_callee)) = action_id.rsplit_once(':') else {
        return Err(edit_error(
            "bad_request",
            "Canvas action id has no checked source callee",
        ));
    };
    if descriptor_callee != callee {
        return Err(edit_error(
            "bad_request",
            "Canvas action callee does not match its checked descriptor",
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
    let formatted = jet_driver::Formatter::format_source(&changed)
        .map_err(|diags| diagnostics_error(path, src, &diags))?;
    let tmp = write_canvas_check_file(path, &formatted)
        .map_err(|e| edit_error("io", &e.to_string()))?;
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

fn write_canvas_check_file(path: &Path, source: &str) -> std::io::Result<PathBuf> {
    let tmp = temp_canvas_check_path(path);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0o400000);
    }
    let mut file = options.open(&tmp)?;
    if let Err(error) = std::io::Write::write_all(&mut file, source.as_bytes()) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(tmp)
}

pub(super) fn apply_insert_structural(
    path: &Path,
    src: &str,
    graph_id: &str,
    op: &str,
    wire_origin_pin_id: Option<&str>,
    wire_target_pin: Option<&str>,
) -> Result<String, String> {
    let projection = project_file(path).map_err(|diags| diagnostics_error(path, src, &diags))?;
    let Some(anchor) = projection
        .graph_anchors
        .iter()
        .find(|a| a.graph_id == graph_id)
    else {
        return Err(edit_error("not_found", "Canvas graph no longer exists"));
    };
    let (insert, indent) = structural_insert_target(
        src,
        graph_id,
        &projection,
        anchor,
        wire_origin_pin_id,
        wire_target_pin,
    )?;
    let stmt = match op {
        "insert_branch" => format!(
            "{indent}if true {{\n{indent}    print(\"branch\")\n{indent}}} else {{\n{indent}    print(\"else\")\n{indent}}}\n"
        ),
        "insert_switch" => format!(
            "{indent}if 0 == {{\n{indent}    0 -> {{ print(\"case\") }}\n{indent}    else -> {{ print(\"else\") }}\n{indent}}}\n"
        ),
        "insert_loop" => format!("{indent}loop {{\n{indent}    break\n{indent}}}\n"),
        "insert_fallible_rail" => {
            if !anchor.fallible {
                return Err(edit_error("unavailable", "needs a fallible function"));
            }
            format!(
                "{indent}fallible_value :: Int.parse(\"1\")\n{indent}unwrapped :: fallible_value ?? 0\n"
            )
        }
        _ => return Err(edit_error("unsupported", "unknown Canvas structural operation")),
    };
    let changed = FixEngine::apply_edits(
        src,
        &[edit(
            SourceSpan {
                start: insert,
                end: insert,
            },
            &stmt,
        )],
    )
    .map_err(|_| edit_error("overlap", "Canvas structural insert overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn structural_insert_target(
    src: &str,
    graph_id: &str,
    projection: &super::schema_api::Projection,
    anchor: &super::schema_api::GraphEditAnchor,
    wire_origin_pin_id: Option<&str>,
    wire_target_pin: Option<&str>,
) -> Result<(usize, String), String> {
    let Some(pin_id) = wire_origin_pin_id else {
        if wire_target_pin.is_some() {
            return Err(edit_error(
                "bad_request",
                "Canvas structural wire origin is missing",
            ));
        }
        return Ok((anchor.insert_offset, "    ".to_string()));
    };
    let Some(target) = wire_target_pin else {
        return Err(edit_error(
            "bad_request",
            "Canvas structural wire target is missing",
        ));
    };
    if target != "exec" {
        return Err(edit_error(
            "bad_request",
            "Canvas structural nodes connect through the exec pin",
        ));
    }
    let (node_id, direction) = pin_id
        .rsplit_once(":input:")
        .map(|(node, _)| (node, "input"))
        .or_else(|| {
            pin_id
                .rsplit_once(":output:")
                .map(|(node, _)| (node, "output"))
        })
        .ok_or_else(|| edit_error("not_found", "Canvas insertion pin no longer exists"))?;
    let pin_marker = format!("\"pin_id\":{}", json_str(pin_id));
    if !projection.json.contains(&pin_marker) {
        return Err(edit_error(
            "not_found",
            "Canvas insertion pin no longer exists",
        ));
    }
    let pin_is_exec = projection.json.split(&pin_marker).skip(1).any(|chunk| {
        chunk
            .split("\"pin_id\":")
            .next()
            .is_some_and(|pin| pin.contains("\"type\":\"exec\""))
    });
    if !pin_is_exec {
        return Err(edit_error(
            "bad_request",
            "Canvas structural nodes connect through the exec pin",
        ));
    }
    let node = projection
        .node_refs
        .iter()
        .filter(|node| node.graph_id == graph_id)
        .filter(|node| node.node_id == node_id)
        .max_by_key(|node| node.span.end.saturating_sub(node.span.start))
        .ok_or_else(|| edit_error("not_found", "Canvas insertion pin no longer exists"))?;
    if node.node_id.ends_with(":entry") {
        return Ok((anchor.insert_offset, "    ".to_string()));
    }
    let insert = if direction == "input" {
        line_start(src, node.span.start)
    } else {
        line_after(src, node.span.end)
    };
    let indent = indentation_at(src, node.span.start);
    let indent = if indent.is_empty() {
        "    ".to_string()
    } else {
        indent
    };
    Ok((insert, indent))
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

pub(super) fn apply_delete_comment_region(
    path: &Path,
    src: &str,
    region_id: &str,
) -> Result<String, String> {
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
    if start >= end || end > src.len() {
        return Err(edit_error(
            "bad_request",
            "Canvas collapse needs one valid source span",
        ));
    }
    let selected = validated_collapse_selection(path, src, graph_id, SourceSpan { start, end })?;
    if canvas_collapse_hints(src)
        .iter()
        .any(|hint| hint.anchor.start == selected.start && hint.anchor.end == selected.end)
    {
        return Err(edit_error(
            "conflict",
            "Canvas selection is already collapsed",
        ));
    }
    let insert_at = selected.end;
    let indent = indentation_at(src, insert_at.min(src.len()));
    let comment = format!(
        "{indent}// canvas:collapse span={}..{} title={}\n",
        selected.start,
        selected.end,
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

fn validated_collapse_selection(
    path: &Path,
    src: &str,
    graph_id: &str,
    requested: SourceSpan,
) -> Result<SourceSpan, String> {
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
    let first = locs
        .iter()
        .find(|loc| {
            requested.start == loc.anchor.start
                || requested.start == loc.source.start
                || requested.start == loc.full.start
        })
        .or_else(|| {
            // Subjectless guards render the condition span, not the `if` token.
            // Accept that graph boundary and let the block comparison below make
            // the final safety decision.
            locs.iter().find(|loc| {
                loc.is_guard
                    && loc.guard_span.is_some_and(|span| {
                        requested.start >= span.start && requested.start < span.end
                    })
            })
        });
    let last = locs
        .iter()
        .find(|loc| {
            requested.end == loc.anchor.end
                || requested.end == loc.source.end
                || requested.end == loc.full.end
        })
        .or_else(|| {
            // Call nodes may expose the callee span while the checked statement
            // owns the complete call expression. Treat an endpoint inside that
            // expression as the graph's statement boundary.
            locs.iter().find(|loc| {
                loc.is_expr && requested.end > loc.source.start && requested.end <= loc.source.end
            })
        });
    let (Some(first), Some(last)) = (first, last) else {
        return Err(edit_error(
            "bad_request",
            "Canvas collapse must select whole statements from this graph",
        ));
    };
    if first.block != last.block || first.index > last.index {
        return Err(edit_error(
            "cross_block",
            "Canvas collapse cannot cross a block boundary",
        ));
    }
    Ok(SourceSpan {
        start: first.full.start,
        end: last.full.end,
    })
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
    let helper = format!("fn {function}({signature}) {ret_type} -> {{\n    return {expr}\n}}\n\n");
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
    source: SourceSpan,
    full: SourceSpan,
    block: Vec<usize>,
    index: usize,
    is_guard: bool,
    is_expr: bool,
    guard_span: Option<SourceSpan>,
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
    let Some(moved_loc) = locs
        .iter()
        .find(|loc| same_span(loc.anchor, moved))
        .cloned()
    else {
        return Err(edit_error(
            "not_found",
            "Canvas step to move no longer exists",
        ));
    };
    let Some(anchor_loc) = locs
        .iter()
        .find(|loc| same_span(loc.anchor, anchor))
        .cloned()
    else {
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

/// Build one source-backed execution convergence edit.
///
/// The browser owns the gesture and the preview choice. It does not own source
/// construction: this function resolves both pins against the checked AST,
/// creates the ordinary Jet helper/call text, and sends the result through the
/// same formatter, sema check, and atomic writer as every other Canvas edit.
pub(super) fn apply_exec_convergence(
    path: &Path,
    src: &str,
    graph_id: &str,
    from_span: SourceSpan,
    target_span: SourceSpan,
    from_pin_name: &str,
    strategy: &str,
    function: &str,
    helper_name: Option<&str>,
) -> Result<String, String> {
    validate_ident(function)?;
    if !matches!(strategy, "extract" | "helper" | "duplicate") {
        return Err(edit_error(
            "bad_request",
            "Canvas convergence strategy must be extract, helper, or duplicate",
        ));
    }
    if same_span(from_span, target_span) {
        return Err(edit_error(
            "bad_request",
            "Canvas convergence needs two different source pins",
        ));
    }

    let func = checked_func_for_graph(path, src, graph_id)?;
    let Some(target) = find_expression_selection(&func.body, target_span, src) else {
        return Err(edit_error(
            "not_found",
            "Canvas convergence target source selection no longer exists",
        ));
    };
    let target_text = src
        .get(target.span.start..target.span.end)
        .ok_or_else(|| edit_error("bad_request", "Canvas convergence target span is stale"))?
        .trim()
        .to_string();
    if target_text.is_empty() {
        return Err(edit_error(
            "ambiguous",
            "Canvas needs one or more complete ordinary source statements to converge",
        ));
    }

    let Some(insertion) = find_exec_insertion(src, &func.body, from_span, from_pin_name)? else {
        return Err(edit_error(
            "not_found",
            "Canvas convergence source path no longer exists",
        ));
    };
    if insertion.branch && target.span.start >= insertion.owner.end {
        return Err(edit_error(
            "structured_join",
            "This source already has a structured join after the branch; keep the downstream step single",
        ));
    }

    let params = convergence_params(path, target.span)?;
    let args = params
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let call_name = if strategy == "helper" {
        helper_name.unwrap_or(function)
    } else {
        function
    };
    let call = format!("{call_name}({args})");
    let incoming_source = if strategy == "duplicate" {
        target_text.clone()
    } else {
        call.clone()
    };

    let bundle = checked_bundle(path, src)?;
    let exact_helper = exact_body_helper_name(bundle, src, &target_text, &func.name);
    let mut edits = Vec::new();
    let insertion_text = format!("{}{}\n", insertion.indent, incoming_source);
    edits.push(edit(
        SourceSpan {
            start: insertion.offset,
            end: insertion.offset,
        },
        &insertion_text,
    ));

    match strategy {
        "helper" => {
            let Some(helper_name) = helper_name.filter(|name| !name.is_empty()) else {
                return Err(edit_error(
                    "bad_request",
                    "Canvas exact-body helper choice has no helper name",
                ));
            };
            validate_ident(helper_name)?;
            if exact_helper.as_deref() != Some(helper_name) {
                return Err(edit_error(
                    "stale",
                    "The selected helper body no longer matches the converged source",
                ));
            }
        }
        "extract" => {
            if function_name_exists(bundle, function) {
                return Err(edit_error(
                    "bad_request",
                    "Canvas helper name already exists; choose another name",
                ));
            }
            let signature = params
                .iter()
                .map(|(name, ty)| format!("{name}: {ty}"))
                .collect::<Vec<_>>()
                .join(", ");
            let body = target_text
                .lines()
                .map(|line| format!("    {}\n", line.trim()))
                .collect::<String>();
            let helper = format!("fn {function}({signature}) {{\n{body}}}\n\n");
            edits.push(edit(SourceSpan { start: 0, end: 0 }, &helper));
        }
        "duplicate" => {}
        _ => unreachable!(),
    }

    if strategy != "duplicate" {
        let suffix = src
            .get(target.span.end..target.full.end)
            .ok_or_else(|| edit_error("bad_request", "Canvas convergence target line is stale"))?;
        let replacement = format!(
            "{}{}{}",
            indentation_at(src, target.full.start),
            call,
            suffix
        );
        edits.push(edit(target.full, &replacement));
    }

    let changed = FixEngine::apply_edits(src, &edits)
        .map_err(|_| edit_error("overlap", "Canvas convergence source edits overlapped"))?;
    write_checked_formatted(path, src, &changed)
}

fn convergence_params(path: &Path, target: SourceSpan) -> Result<Vec<(String, String)>, String> {
    let index = jet_semindex::open(path).map_err(|error| {
        edit_error(
            "check",
            &format!("Canvas could not resolve convergence captures: {error}"),
        )
    })?;
    let mut references = index
        .references()
        .iter()
        .filter(|reference| {
            reference.span.start >= target.start && reference.span.end <= target.end
        })
        .collect::<Vec<_>>();
    references.sort_by_key(|reference| (reference.span.start, reference.span.end));

    let mut params = Vec::new();
    for reference in references {
        let Some(target_def) = reference.target.as_ref() else {
            continue;
        };
        let Some(definition) = index.definitions().iter().find(|definition| {
            definition.module_path == target_def.module_path
                && definition.def_span == target_def.def_span
        }) else {
            continue;
        };
        let ty = match &definition.kind {
            jet_semindex::SymbolKind::Local { ty: Some(ty), .. }
            | jet_semindex::SymbolKind::Param { ty } => ty.clone(),
            jet_semindex::SymbolKind::Local { ty: None, .. } => {
                return Err(edit_error(
                    "ambiguous",
                    "Canvas could not preserve a convergence capture type",
                ));
            }
            _ => continue,
        };
        if !params.iter().any(|(name, _)| name == &reference.name) {
            params.push((reference.name.clone(), ty));
        }
    }
    Ok(params)
}

#[derive(Clone, Copy)]
struct ExpressionSelection {
    span: SourceSpan,
    full: SourceSpan,
}

#[derive(Clone)]
struct ExecInsertion {
    offset: usize,
    indent: String,
    owner: SourceSpan,
    branch: bool,
}

fn find_expression_selection(
    stmts: &[Stmt],
    target: SourceSpan,
    src: &str,
) -> Option<ExpressionSelection> {
    if let Some(found) = find_expression_selection_in_block(stmts, target, src) {
        return Some(found);
    }
    for stmt in stmts {
        if let Some(found) = find_expression_selection_in_children(stmt, target, src) {
            return Some(found);
        }
    }
    None
}

fn find_expression_selection_in_block(
    stmts: &[Stmt],
    target: SourceSpan,
    src: &str,
) -> Option<ExpressionSelection> {
    let mut first = None;
    let mut last = None;
    for (index, stmt) in stmts.iter().enumerate() {
        let Stmt::Expr(expr) = stmt else {
            continue;
        };
        let anchor: SourceSpan = expr.span().into();
        let expr_span = source_expression_span(src, expr);
        if same_span(anchor, target)
            || same_span(expr_span, target)
            || span_contains(expr_span, target)
            || span_contains(target, expr_span)
            || source_spans_overlap(expr_span, target)
        {
            first.get_or_insert(index);
            last = Some(index);
        }
    }
    let (Some(first), Some(last)) = (first, last) else {
        return None;
    };
    let mut selected = Vec::with_capacity(last - first + 1);
    for stmt in &stmts[first..=last] {
        let Stmt::Expr(expr) = stmt else {
            return None;
        };
        selected.push(source_expression_span(src, expr));
    }
    let span = SourceSpan {
        start: selected.first()?.start,
        end: selected.last()?.end,
    };
    if selected.len() > 1 && (span.start < target.start || span.end > target.end) {
        return None;
    }
    Some(ExpressionSelection {
        span,
        full: SourceSpan {
            start: line_start(src, span.start),
            end: line_after(src, span.end),
        },
    })
}

fn source_expression_span(src: &str, expr: &Expr) -> SourceSpan {
    let start = match expr {
        Expr::Call(call) => call.name_span.start,
        Expr::MethodCall { receiver, .. } => source_expression_span(src, receiver).start,
        Expr::Try(inner, ..) | Expr::OrFallback { value: inner, .. } => {
            source_expression_span(src, inner).start
        }
        Expr::Paren(_, span) => span.start,
        _ => expr.span().start,
    };
    let fallback_end = expr.span().end;
    let end = match expr {
        Expr::Call(call) => call_source_end(src, call.name_span.end, fallback_end),
        Expr::MethodCall { method_span, .. } => call_source_end(src, method_span.end, fallback_end),
        Expr::Try(inner, span, ..) => {
            let inner_end = source_expression_span(src, inner).end;
            let mut end = inner_end.max(span.end);
            if src.as_bytes().get(end) == Some(&b'?') {
                end += 1;
            }
            end
        }
        _ => fallback_end,
    };
    SourceSpan {
        start,
        end: end.max(start),
    }
}

fn call_source_end(src: &str, name_end: usize, fallback_end: usize) -> usize {
    let Some(open) = find_unquoted_char(src, name_end, '(') else {
        return fallback_end;
    };
    matching_delimiter(src, open, '(', ')').map_or(fallback_end, |close| close.saturating_add(1))
}

fn span_contains(outer: SourceSpan, inner: SourceSpan) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

fn source_spans_overlap(a: SourceSpan, b: SourceSpan) -> bool {
    a.start < b.end && b.start < a.end
}

fn find_expression_selection_in_children(
    stmt: &Stmt,
    target: SourceSpan,
    src: &str,
) -> Option<ExpressionSelection> {
    match stmt {
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_expression_selection(body, target, src),
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => arms
            .iter()
            .find_map(|arm| find_expression_selection(&arm.body, target, src))
            .or_else(|| {
                else_body
                    .as_deref()
                    .and_then(|body| find_expression_selection(body, target, src))
            }),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => find_expression_selection(then_body, target, src).or_else(|| {
            else_body
                .as_deref()
                .and_then(|body| find_expression_selection(body, target, src))
        }),
        _ => None,
    }
}

fn find_exec_insertion(
    src: &str,
    stmts: &[Stmt],
    node_span: SourceSpan,
    pin_name: &str,
) -> Result<Option<ExecInsertion>, String> {
    for stmt in stmts {
        if exec_branch_node_matches(stmt, node_span) {
            let body = matching_exec_branch(stmt, node_span, pin_name).ok_or_else(|| {
                edit_error(
                    "ambiguous",
                    "Canvas convergence pin has no source-backed branch body",
                )
            })?;
            let owner = branch_source_span(src, stmt);
            return branch_exec_insertion(src, body, owner).map(Some);
        }
        let source = stmt_source_span(stmt);
        let anchor: SourceSpan = stmt_canvas_anchor(stmt).into();
        if same_span(source, node_span) || same_span(anchor, node_span) {
            if execution_terminates(stmt) {
                return Err(edit_error(
                    "ambiguous",
                    "Canvas cannot add a convergence step after an early exit",
                ));
            }
            let full = stmt_text_span(src, stmt);
            return Ok(Some(ExecInsertion {
                offset: full.end,
                indent: indentation_at(src, full.start),
                owner: full,
                branch: false,
            }));
        }
        if let Some(found) = find_exec_insertion_in_children(src, stmt, node_span, pin_name)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn exec_branch_node_matches(stmt: &Stmt, node_span: SourceSpan) -> bool {
    match stmt {
        Stmt::Switch { arms, span, .. } | Stmt::ComptimeSwitch { arms, span, .. } => {
            same_span((*span).into(), node_span)
                || arms
                    .iter()
                    .any(|arm| same_span(arm.cond.span().into(), node_span))
        }
        Stmt::ComptimeIf { span, .. } => same_span((*span).into(), node_span),
        _ => false,
    }
}

fn branch_source_span(src: &str, stmt: &Stmt) -> SourceSpan {
    let line = line_start(src, stmt.span().start);
    let keyword = find_word(src, line, "if").unwrap_or(stmt.span().start);
    let Some(open) = find_unquoted_char(src, keyword, '{') else {
        return stmt.span().into();
    };
    let mut close = matching_delimiter(src, open, '{', '}').unwrap_or(stmt.span().end);
    loop {
        let Some(else_start) = find_word(src, close + 1, "else") else {
            break;
        };
        let between = &src[close + 1..else_start];
        if !between.chars().all(char::is_whitespace) {
            break;
        }
        let Some(next_open) = find_unquoted_char(src, else_start + "else".len(), '{') else {
            break;
        };
        let Some(next_close) = matching_delimiter(src, next_open, '{', '}') else {
            break;
        };
        close = next_close;
    }
    SourceSpan {
        start: keyword,
        end: close.saturating_add(1),
    }
}

fn find_word(src: &str, start: usize, word: &str) -> Option<usize> {
    let mut cursor = start.min(src.len());
    while let Some(relative) = src[cursor..].find(word) {
        let found = cursor + relative;
        let before = found.checked_sub(1).and_then(|i| src.as_bytes().get(i));
        let after = src.as_bytes().get(found + word.len());
        if !before.is_some_and(u8::is_ascii_alphanumeric)
            && !before.is_some_and(|byte| *byte == b'_')
            && !after.is_some_and(u8::is_ascii_alphanumeric)
            && !after.is_some_and(|byte| *byte == b'_')
        {
            return Some(found);
        }
        cursor = found + word.len();
    }
    None
}

fn find_unquoted_char(src: &str, start: usize, wanted: char) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut cursor = start.min(src.len());
    while cursor < src.len() {
        let ch = src[cursor..].chars().next()?;
        let width = ch.len_utf8();
        let next = src[cursor + width..].chars().next();
        if line_comment {
            if ch == '\n' {
                line_comment = false;
            }
        } else if let Some(quoted) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quoted {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '/' && next == Some('/') {
            line_comment = true;
            cursor += width;
        } else if ch == wanted {
            return Some(cursor);
        }
        cursor += width;
    }
    None
}

fn matching_delimiter(src: &str, open: usize, opening: char, closing: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut cursor = open.min(src.len());
    while cursor < src.len() {
        let ch = src[cursor..].chars().next()?;
        let width = ch.len_utf8();
        let next = src[cursor + width..].chars().next();
        if line_comment {
            if ch == '\n' {
                line_comment = false;
            }
        } else if let Some(quoted) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quoted {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '/' && next == Some('/') {
            line_comment = true;
            cursor += width;
        } else if ch == opening {
            depth += 1;
        } else if ch == closing {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += width;
    }
    None
}

fn find_exec_insertion_in_children(
    src: &str,
    stmt: &Stmt,
    node_span: SourceSpan,
    pin_name: &str,
) -> Result<Option<ExecInsertion>, String> {
    match stmt {
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_exec_insertion(src, body, node_span, pin_name),
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            for arm in arms {
                if let Some(found) = find_exec_insertion(src, &arm.body, node_span, pin_name)? {
                    return Ok(Some(found));
                }
            }
            else_body
                .as_deref()
                .map(|body| find_exec_insertion(src, body, node_span, pin_name))
                .transpose()
                .map(|found| found.flatten())
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            if let Some(found) = find_exec_insertion(src, then_body, node_span, pin_name)? {
                return Ok(Some(found));
            }
            else_body
                .as_deref()
                .map(|body| find_exec_insertion(src, body, node_span, pin_name))
                .transpose()
                .map(|found| found.flatten())
        }
        _ => Ok(None),
    }
}

fn matching_exec_branch<'a>(
    stmt: &'a Stmt,
    node_span: SourceSpan,
    pin_name: &str,
) -> Option<&'a [Stmt]> {
    if let Stmt::ComptimeIf {
        then_body,
        else_body,
        span,
        ..
    } = stmt
    {
        if !same_span((*span).into(), node_span) {
            return None;
        }
        return match pin_name {
            "then" => Some(then_body.as_slice()),
            "else" => else_body.as_deref(),
            _ => None,
        };
    }
    let (arms, else_body, span) = match stmt {
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
        } => (arms, else_body, *span),
        _ => return None,
    };
    let node_matches = same_span(span.into(), node_span)
        || arms
            .iter()
            .any(|arm| same_span(arm.cond.span().into(), node_span));
    if !node_matches {
        return None;
    }
    if pin_name == "else" {
        return else_body.as_deref();
    }
    if pin_name == "then" {
        return arms.first().map(|arm| arm.body.as_slice());
    }
    let index = pin_name.strip_prefix("arm")?.parse::<usize>().ok()?;
    arms.get(index.checked_sub(1)?)
        .map(|arm| arm.body.as_slice())
}

fn branch_exec_insertion(
    src: &str,
    body: &[Stmt],
    owner: SourceSpan,
) -> Result<ExecInsertion, String> {
    let Some(last) = body.last() else {
        return Err(edit_error(
            "ambiguous",
            "Canvas needs a non-empty source-backed branch body to converge",
        ));
    };
    if execution_terminates(last)
        || matches!(
            last,
            Stmt::While { .. }
                | Stmt::For { .. }
                | Stmt::Loop { .. }
                | Stmt::CountedLoop { .. }
                | Stmt::Switch { .. }
                | Stmt::ComptimeSwitch { .. }
                | Stmt::ComptimeIf { .. }
        )
    {
        return Err(edit_error(
            "ambiguous",
            "Canvas cannot prove a convergence path after this branch body",
        ));
    }
    let full = stmt_text_span(src, last);
    let inline = full.end > owner.end;
    let (offset, indent) = if inline {
        let last_end = match last {
            Stmt::Expr(expr) => source_expression_span(src, expr).end,
            _ => stmt_source_span(last).end,
        };
        let Some(close) =
            find_unquoted_char(src, last_end, '}').filter(|offset| *offset < owner.end)
        else {
            return Err(edit_error(
                "ambiguous",
                "Canvas could not locate the source-backed branch body boundary",
            ));
        };
        // The formatter may keep a one-statement branch on the declaration line.
        // Insert a new line before its closing brace, then let the formatter
        // restore the canonical block layout.
        (close, format!("\n{}    ", indentation_at(src, owner.start)))
    } else {
        (full.end, indentation_at(src, full.start))
    };
    Ok(ExecInsertion {
        offset,
        indent,
        owner,
        branch: true,
    })
}

fn execution_terminates(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Return(..)
            | Stmt::Break(..)
            | Stmt::BreakValue(..)
            | Stmt::BreakLabel(..)
            | Stmt::BreakLabelValue(..)
            | Stmt::Continue(..)
            | Stmt::ContinueLabel(..)
    )
}

fn exact_body_helper_name(
    bundle: &AST::ProgramBundle,
    src: &str,
    target: &str,
    current: &str,
) -> Option<String> {
    fn in_items(items: &[Item], src: &str, target: &str, current: &str) -> Option<String> {
        for item in items {
            match item {
                Item::Func(func) if func.name != current => {
                    if expression_body_source(src, &func.body)
                        .is_some_and(|body| formatted_expression_matches(&body, target))
                    {
                        return Some(func.name.clone());
                    }
                }
                Item::Struct(structure) => {
                    if let Some(found) = structure.methods.iter().find_map(|method| {
                        if method.name == current {
                            return None;
                        }
                        expression_body_source(src, &method.body).and_then(|body| {
                            formatted_expression_matches(&body, target).then(|| method.name.clone())
                        })
                    }) {
                        return Some(found);
                    }
                }
                Item::Impl(implementation) => {
                    if let Some(found) = implementation.methods.iter().find_map(|method| {
                        if method.name == current {
                            return None;
                        }
                        expression_body_source(src, &method.body).and_then(|body| {
                            formatted_expression_matches(&body, target).then(|| method.name.clone())
                        })
                    }) {
                        return Some(found);
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        if let Some(found) = in_items(body, src, target, current) {
                            return Some(found);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    bundle
        .modules
        .iter()
        .find_map(|module| in_items(&module.items, src, target, current))
}

fn expression_body_source(src: &str, body: &[Stmt]) -> Option<String> {
    let mut spans = Vec::with_capacity(body.len());
    for stmt in body {
        let Stmt::Expr(expr) = stmt else {
            return None;
        };
        spans.push(source_expression_span(src, expr));
    }
    let start = spans.first()?.start;
    let end = spans.last()?.end;
    Some(
        source_snippet(src, SourceSpan { start, end })
            .trim()
            .to_string(),
    )
}

fn source_snippet(src: &str, span: SourceSpan) -> String {
    src.get(span.start..span.end)
        .unwrap_or_default()
        .to_string()
}

fn formatted_expression_matches(left: &str, right: &str) -> bool {
    let wrap = |expr: &str| {
        let candidate = format!("fn canvas_match() {{\n    {}\n}}\n", expr.trim());
        jet_driver::Formatter::format_source(&candidate).unwrap_or_else(|_| candidate.clone())
    };
    let left = wrap(left);
    let right = wrap(right);
    left == right || normalized_source_shape(&left) == normalized_source_shape(&right)
}

fn normalized_source_shape(source: &str) -> String {
    let mut normalized = String::with_capacity(source.len());
    let mut quote = None;
    let mut escaped = false;
    for ch in source.chars() {
        if let Some(quoted) = quote {
            normalized.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quoted {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
            normalized.push(ch);
        } else if !ch.is_whitespace() {
            normalized.push(ch);
        }
    }
    normalized
}

fn function_name_exists(bundle: &AST::ProgramBundle, name: &str) -> bool {
    fn in_items(items: &[Item], name: &str) -> bool {
        items.iter().any(|item| match item {
            Item::Func(func) => func.name == name,
            Item::Struct(structure) => structure.methods.iter().any(|method| method.name == name),
            Item::Impl(implementation) => implementation
                .methods
                .iter()
                .any(|method| method.name == name),
            Item::CodeModule(module) => module
                .body
                .as_deref()
                .is_some_and(|body| in_items(body, name)),
            _ => false,
        })
    }
    bundle
        .modules
        .iter()
        .any(|module| in_items(&module.items, name))
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
    find_pattern_target(src, &func.body, node_span, &mut target);
    let Some(target) = target else {
        return Err(edit_error(
            "not_found",
            "Canvas pattern node no longer exists",
        ));
    };
    let body = fresh_arm_body(func);
    let changed = match target {
        PatternTarget::Classic {
            arm,
            else_body,
            span,
        } => add_arm_to_classic_switch(src, arm, else_body, span, &head, &body)?,
        PatternTarget::Switch {
            arms,
            else_body,
            span,
            ..
        } => {
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
    if !pattern_span_belongs_to_graph(src, &func.body, pattern_span) {
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

pub(super) fn apply_toggle_switch_state(
    path: &Path,
    src: &str,
    graph_id: &str,
    node_span: SourceSpan,
) -> Result<String, String> {
    let func = checked_func_for_graph(path, src, graph_id)?;
    match find_switched_node(&func.body, node_span) {
        Some(SwitchedNode::Marker(marker_span)) => {
            let changed = FixEngine::apply_edits(src, &[edit(marker_span, "")])
                .map_err(|_| edit_error("overlap", "Canvas statement-state toggle overlapped"))?;
            write_checked_formatted(path, src, &changed)
        }
        Some(SwitchedNode::Nested) => Err(edit_error(
            "bad_request",
            "Canvas node is already inside a statement state",
        )),
        None if !statement_contains_span(&func.body, node_span)
            || same_span(func.name_span.into(), node_span) =>
        {
            Err(edit_error(
                "not_found",
                "Canvas statement node no longer exists",
            ))
        }
        None => {
            let insert = line_start(src, node_span.start);
            let changed = FixEngine::apply_edits(
                src,
                &[edit(
                    SourceSpan {
                        start: insert,
                        end: insert,
                    },
                    "#Off ",
                )],
            )
            .map_err(|_| edit_error("overlap", "Canvas statement-state toggle overlapped"))?;
            write_checked_formatted(path, src, &changed)
        }
    }
}

enum SwitchedNode {
    Marker(SourceSpan),
    Nested,
}

fn find_switched_node(stmts: &[Stmt], node_span: SourceSpan) -> Option<SwitchedNode> {
    for stmt in stmts {
        match stmt {
            Stmt::Switched { marker, body, span } => {
                let switched_span: SourceSpan = (*span).into();
                if same_span(switched_span, node_span) {
                    return Some(SwitchedNode::Marker(marker.span.into()));
                }
                if span_within(node_span, switched_span) {
                    return Some(SwitchedNode::Nested);
                }
                if let Some(found) = find_switched_node(body, node_span) {
                    return Some(found);
                }
            }
            _ => {
                if let Some(found) = find_switched_node_in_children(stmt, node_span) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_switched_node_in_children(stmt: &Stmt, node_span: SourceSpan) -> Option<SwitchedNode> {
    match stmt {
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_switched_node(body, node_span),
        Stmt::CountedLoop { step, body, .. } => step
            .as_deref()
            .and_then(|step| find_switched_node(std::slice::from_ref(step), node_span))
            .or_else(|| find_switched_node(body, node_span)),
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            for arm in arms {
                if let Some(found) = find_switched_node(&arm.body, node_span) {
                    return Some(found);
                }
            }
            else_body
                .as_deref()
                .and_then(|body| find_switched_node(body, node_span))
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => find_switched_node(then_body, node_span).or_else(|| {
            else_body
                .as_deref()
                .and_then(|body| find_switched_node(body, node_span))
        }),
        _ => None,
    }
}

fn statement_contains_span(stmts: &[Stmt], node_span: SourceSpan) -> bool {
    stmts
        .iter()
        .any(|stmt| span_within(node_span, stmt.span().into()))
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
    let (span, item_spans) = match target {
        MultiInputTarget::List { span, item_spans } => (span, item_spans),
    };
    let list_source = src
        .get(span.start..span.end)
        .ok_or_else(|| edit_error("bad_request", "Canvas multi-input source span is stale"))?;
    let insert = if let Some(item) = item_spans.last() {
        item.end
    } else {
        list_source
            .rfind(']')
            .map(|idx| span.start + idx)
            .ok_or_else(|| edit_error("bad_request", "Canvas multi-input source span is stale"))?
    };
    let prefix = if item_spans.is_empty() { "" } else { ", " };
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
    Classic {
        arm: &'a AST::SwitchArm,
        else_body: Option<&'a Vec<Stmt>>,
        span: SourceSpan,
    },
    Switch {
        arms: &'a [AST::SwitchArm],
        else_body: Option<&'a Vec<Stmt>>,
        span: SourceSpan,
    },
}

fn find_pattern_target<'a>(
    src: &str,
    stmts: &'a [Stmt],
    node_span: SourceSpan,
    out: &mut Option<PatternTarget<'a>>,
) {
    if out.is_some() {
        return;
    }
    for stmt in stmts {
        match stmt {
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
                ..
            } => {
                let classic = AST::is_subjectless_guard(subject, *span)
                    && arms.first().is_some_and(|arm| {
                        AST::uses_classic_if_spelling(src, *span, arm.cond.span())
                    });
                if classic {
                    if let Some(arm) = arms.first() {
                        if (same_span(arm.cond.span().into(), node_span)
                            || same_span((*span).into(), node_span))
                            && matches!(arm.cond, Expr::PatternTest { .. })
                        {
                            *out = Some(PatternTarget::Classic {
                                arm,
                                else_body: else_body.as_ref(),
                                span: stmt_text_span(src, stmt),
                            });
                            return;
                        }
                    }
                } else if same_span((*span).into(), node_span) {
                    *out = Some(PatternTarget::Switch {
                        arms,
                        else_body: else_body.as_ref(),
                        span: (*span).into(),
                    });
                    return;
                }
                for arm in arms {
                    find_pattern_target(src, &arm.body, node_span, out);
                }
                if let Some(body) = else_body {
                    find_pattern_target(src, body, node_span, out);
                }
            }
            Stmt::ComptimeSwitch {
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
                    find_pattern_target(src, &arm.body, node_span, out);
                }
                if let Some(body) = else_body {
                    find_pattern_target(src, body, node_span, out);
                }
            }
            _ => find_pattern_target_in_children(src, stmt, node_span, out),
        }
    }
}

fn find_pattern_target_in_children<'a>(
    src: &str,
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
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_pattern_target(src, body, node_span, out),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            find_pattern_target(src, then_body, node_span, out);
            if let Some(body) = else_body {
                find_pattern_target(src, body, node_span, out);
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
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
            } => {
                for arm in arms {
                    let classic = AST::is_subjectless_guard(subject, *span)
                        && AST::uses_classic_if_spelling(src, *span, arm.cond.span());
                    if same_span(switch_arm_edit_span(src, subject, *span, arm), pattern_span) {
                        *out = Some(RemoveArmInfo {
                            remove_span: if classic {
                                stmt_text_span(src, stmt)
                            } else {
                                stmt_arm_text_span(src, arm)
                            },
                            only_arm_without_else: classic
                                || (arms.len() == 1 && else_body.is_none()),
                        });
                        return;
                    }
                    find_pattern_arm_remove_span(src, &arm.body, pattern_span, out);
                }
                if let Some(body) = else_body {
                    find_pattern_arm_remove_span(src, body, pattern_span, out);
                }
            }
            Stmt::ComptimeSwitch {
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
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
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

fn pattern_span_belongs_to_graph(src: &str, stmts: &[Stmt], pattern_span: SourceSpan) -> bool {
    let mut found = false;
    find_pattern_span(src, stmts, pattern_span, &mut found);
    found
}

fn find_pattern_span(src: &str, stmts: &[Stmt], pattern_span: SourceSpan, found: &mut bool) {
    if *found {
        return;
    }
    for stmt in stmts {
        match stmt {
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
            } => {
                for arm in arms {
                    if same_span(switch_arm_edit_span(src, subject, *span, arm), pattern_span) {
                        *found = true;
                        return;
                    }
                    find_pattern_span(src, &arm.body, pattern_span, found);
                }
                if let Some(body) = else_body {
                    find_pattern_span(src, body, pattern_span, found);
                }
            }
            Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    if same_span(arm.cond.span().into(), pattern_span) {
                        *found = true;
                        return;
                    }
                    find_pattern_span(src, &arm.body, pattern_span, found);
                }
                if let Some(body) = else_body {
                    find_pattern_span(src, body, pattern_span, found);
                }
            }
            _ => find_pattern_span_in_children(src, stmt, pattern_span, found),
        }
    }
}

fn find_pattern_span_in_children(
    src: &str,
    stmt: &Stmt,
    pattern_span: SourceSpan,
    found: &mut bool,
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
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_pattern_span(src, body, pattern_span, found),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            find_pattern_span(src, then_body, pattern_span, found);
            if let Some(body) = else_body {
                find_pattern_span(src, body, pattern_span, found);
            }
        }
        _ => {}
    }
}

enum MultiInputTarget {
    List {
        span: SourceSpan,
        item_spans: Vec<SourceSpan>,
    },
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

fn find_multi_input_in_stmt(
    stmt: &Stmt,
    node_span: SourceSpan,
    out: &mut Option<MultiInputTarget>,
) {
    if out.is_some() {
        return;
    }
    match stmt {
        Stmt::Val(b) => find_multi_input_in_expr(&b.init, node_span, out),
        Stmt::Assign { target, value, .. } => {
            find_multi_input_in_lvalue(target, node_span, out);
            find_multi_input_in_expr(value, node_span, out);
        }
        Stmt::Expr(e) | Stmt::BreakValue(e, _) | Stmt::BreakLabelValue(_, _, e, _) => {
            find_multi_input_in_expr(e, node_span, out)
        }
        Stmt::DeferClose { close, .. } => find_multi_input_in_expr(close, node_span, out),
        Stmt::Return(Some(e), _) => find_multi_input_in_expr(e, node_span, out),
        Stmt::Yield(e, _) => find_multi_input_in_expr(e, node_span, out),
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
        Stmt::For { kind, body, .. } => {
            match kind {
                AST::ForKind::Range {
                    start,
                    end,
                    step,
                    exclusive: _,
                } => {
                    find_multi_input_in_expr(start, node_span, out);
                    find_multi_input_in_expr(end, node_span, out);
                    if let Some(step) = step {
                        find_multi_input_in_expr(step, node_span, out);
                    }
                }
                AST::ForKind::In { collection, step } => {
                    find_multi_input_in_expr(collection, node_span, out);
                    if let Some(step) = step {
                        find_multi_input_in_expr(step, node_span, out);
                    }
                }
            }
            find_multi_input_target(body, node_span, out);
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            find_multi_input_in_expr(&init.init, node_span, out);
            find_multi_input_in_expr(cond, node_span, out);
            if let Some(step) = step {
                find_multi_input_in_stmt(step, node_span, out);
            }
            find_multi_input_target(body, node_span, out);
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => find_multi_input_target(body, node_span, out),
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            find_multi_input_in_expr(cond, node_span, out);
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
                item_spans: items.iter().map(|item| item.span().into()).collect(),
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
            base,
            start,
            end,
            range,
            ..
        } => {
            find_multi_input_in_expr(base, node_span, out);
            if let Some(range) = range {
                find_multi_input_in_expr(range, node_span, out);
            } else {
                find_multi_input_in_expr(start, node_span, out);
                find_multi_input_in_expr(end, node_span, out);
            }
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
    let mut target = None;
    find_multi_input_target(stmts, node_span, &mut target);
    match target {
        Some(MultiInputTarget::List { item_spans, .. }) => item_spans
            .iter()
            .any(|item_span| same_span(*item_span, element_span)),
        None => false,
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

fn switch_arm_edit_span(
    src: &str,
    subject: &Expr,
    switch_span: Span,
    arm: &AST::SwitchArm,
) -> SourceSpan {
    if AST::is_subjectless_guard(subject, switch_span)
        && AST::uses_classic_if_spelling(src, switch_span, arm.cond.span())
    {
        if let Expr::PatternTest { pattern, .. } = &arm.cond {
            return span_through_closing_parens(src, pattern.span());
        }
    }
    arm.cond.span().into()
}

fn add_arm_to_classic_switch(
    src: &str,
    arm: &AST::SwitchArm,
    else_body: Option<&Vec<Stmt>>,
    span: SourceSpan,
    new_head: &str,
    fresh_body: &str,
) -> Result<String, String> {
    let Expr::PatternTest {
        subject, pattern, ..
    } = &arm.cond
    else {
        return Err(edit_error(
            "bad_request",
            "Canvas branch is not a pattern branch",
        ));
    };
    let subject_src = snippet(src, subject.span());
    let pattern_span = span_through_closing_parens(src, pattern.span());
    let head = src
        .get(pattern_span.start..pattern_span.end)
        .map(str::to_owned)
        .unwrap_or_else(|| snippet(src, pattern.span()));
    let then_body = block_body_source(src, &arm.body, 2);
    let else_body = if let Some(body) = else_body {
        let body = block_body_source(src, body, 2);
        format!("    else -> {{\n{body}    }}\n")
    } else {
        String::new()
    };
    let replacement = format!(
        "if {subject_src} == {{\n    {head} -> {{\n{then_body}    }}\n    {new_head} -> {{\n        {fresh_body}\n    }}\n{else_body}}}\n"
    );
    FixEngine::apply_edits(src, &[edit(span, &replacement)])
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
        return Err(edit_error(
            "bad_request",
            "Canvas multi-input span is stale",
        ));
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
    while offset < limit
        && src
            .as_bytes()
            .get(offset)
            .is_some_and(u8::is_ascii_whitespace)
    {
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
            source: stmt_source_span(stmt),
            full: stmt_text_span(src, stmt),
            block: block.clone(),
            index,
            is_guard: matches!(stmt, Stmt::Switch { .. } | Stmt::ComptimeSwitch { .. }),
            is_expr: matches!(stmt, Stmt::Expr(_)),
            guard_span: match stmt {
                Stmt::Switch { arms, .. } | Stmt::ComptimeSwitch { arms, .. } => {
                    arms.first().map(|arm| arm.cond.span().into())
                }
                _ => None,
            },
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
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
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
        Stmt::Val(b) => b.name_span,
        Stmt::Assign { target, .. } => target.span(),
        Stmt::Expr(e) => e.span(),
        Stmt::Return(_, span)
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::BreakLabel(_, span)
        | Stmt::ContinueLabel(_, span) => *span,
        Stmt::BreakValue(_, span) | Stmt::BreakLabelValue(_, _, _, span) => *span,
        _ => stmt.span(),
    }
}

fn stmt_text_span(src: &str, stmt: &Stmt) -> SourceSpan {
    let span = match stmt {
        Stmt::Expr(expr) => source_expression_span(src, expr),
        _ => stmt_source_span(stmt),
    };
    SourceSpan {
        start: line_start(src, span.start),
        end: line_after(src, span.end),
    }
}

fn stmt_source_span(stmt: &Stmt) -> SourceSpan {
    match stmt {
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
        Stmt::BreakValue(e, span) | Stmt::BreakLabelValue(_, _, e, span) => SourceSpan {
            start: span.start,
            end: e.span().end,
        },
        _ => stmt.span().into(),
    }
}

fn same_span(a: SourceSpan, b: SourceSpan) -> bool {
    a.start == b.start && a.end == b.end
}

pub(super) fn write_checked_formatted(
    path: &Path,
    before: &str,
    candidate: &str,
) -> Result<String, String> {
    let formatted = jet_driver::Formatter::format_source(candidate)
        .map_err(|diags| diagnostics_error(path, candidate, &diags))?;
    write_checked_candidate(path, before, &formatted)
}

pub(super) fn write_checked_source(
    path: &Path,
    before: &str,
    candidate: &str,
) -> Result<String, String> {
    write_checked_candidate(path, before, candidate)
}

fn write_checked_candidate(path: &Path, before: &str, candidate: &str) -> Result<String, String> {
    let path_str = path.to_string_lossy();
    let abs = canonical_path(path);
    let (diags, _, _) =
        jet_driver::Driver::check_file_with_effect_facts(&path_str, Some((&abs, candidate)), true);
    let errors: Vec<Diagnostic> = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(diagnostics_error(path, candidate, &errors));
    }
    let changed = candidate != before;
    if changed {
        write_source_if_unchanged(path, before, candidate).map_err(|error| match error {
            SourceWriteError::Conflict => edit_error(
                "conflict",
                "source changed while this Canvas edit was prepared",
            ),
            SourceWriteError::Io(error) => edit_error("io", &error.to_string()),
        })?;
    }
    Ok(edit_ok_source(changed, candidate))
}

#[cfg(test)]
mod tests {
    use super::{temp_canvas_check_path, write_canvas_check_file};

    #[cfg(unix)]
    #[test]
    fn canvas_action_check_writer_rejects_existing_symlink() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jet-canvas-action-check-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("main.jet");
        let outside = root.join("outside.jet");
        std::fs::write(&outside, "must survive\n").unwrap();
        let temp = temp_canvas_check_path(&source);
        symlink(&outside, &temp).unwrap();

        assert!(
            write_canvas_check_file(&source, "attacker\n").is_err(),
            "Canvas action validation must not follow a pre-existing temp symlink"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must survive\n");

        let _ = std::fs::remove_dir_all(&root);
    }
}
