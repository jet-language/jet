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

fn apply_create_trait_impl(
    path: &Path,
    src: &str,
    type_name: &str,
    trait_name: &str,
) -> Result<String, String> {
    let path_str = path.to_string_lossy();
    let (diags, bundle) = crate::Driver::check_file(&path_str, None, true);
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
