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
    let authority = canvas_authority_context(path);
    let mut entries = Vec::new();
    let mut project_functions = Vec::new();
    for def in index.definitions() {
        let SymbolKind::Function { params, ret } = &def.kind else {
            continue;
        };
        project_functions.push(project_function_catalog_json(
            def,
            params,
            ret.as_deref(),
            &index,
        ));
        if def.name == "run" {
            continue;
        }
        entries.push(canvas_action_json(def, params, ret.as_deref(), &authority));
    }
    entries.push(canvas_builtin_action_json(
        "print",
        "Print",
        &[("value", "Any")],
        "Void",
        &["\"canvas\""],
        &authority,
    ));
    entries.extend(canvas_core_catalog_action_jsons(src, &authority));
    entries.extend(canvas_command_action_jsons(&authority));
    entries.sort();
    project_functions.sort();
    Ok(query_ok(
        "actions",
        src,
        "[]",
        &format!(
            "\"impact\":null,\"diff\":null,\"actions_schema_version\":{},\"project_functions\":[{}],\"actions\":[{}]",
            ACTION_SCHEMA_VERSION,
            project_functions.join(","),
            entries.join(",")
        ),
    ))
}

fn canvas_core_catalog_query(path: &Path, src: &str, request: &str) -> Result<String, String> {
    let query = json_string_field(request, "query").unwrap_or_default();
    canvas_core_catalog(path, src, &query)
}

fn canvas_core_catalog(_path: &Path, src: &str, query: &str) -> Result<String, String> {
    let catalog = core_catalog_entries(query);
    let modules = catalog
        .iter()
        .map(core_module_json)
        .collect::<Vec<_>>()
        .join(",");
    Ok(query_ok(
        "core_catalog",
        src,
        "[]",
        &format!(
            "\"impact\":null,\"diff\":null,\"catalog_schema_version\":{},\"authority\":[{}],\"writes\":\"none\",\"source\":{},\"modules\":[{}]",
            CORE_CATALOG_SCHEMA_VERSION,
            json_str("canvas.catalog:core.read"),
            json_str("docs/reference/core-library.md"),
            modules
        ),
    ))
}

fn canvas_core_catalog_action_jsons(src: &str, authority: &CanvasAuthority) -> Vec<String> {
    core_catalog_entries("")
        .into_iter()
        .flat_map(|module| {
            let module_path = module.path.clone();
            let source_callee = core_source_callee(src, &module_path);
            module
                .members
                .into_iter()
                .map(move |member| {
                    core_catalog_action_json(&module_path, &source_callee, &member, authority)
                })
        })
        .collect()
}

fn core_catalog_action_json(
    module_path: &str,
    source_callee_prefix: &str,
    member: &CoreCatalogMember,
    authority: &CanvasAuthority,
) -> String {
    let action_id = format!("canvas.core_catalog:{module_path}:{}", member.name);
    let callee = format!("{source_callee_prefix}.{}", member.name);
    let params = core_member_params(module_path, &member.name, &member.signature);
    let default_args = params
        .iter()
        .map(|(_, ty)| json_str(&default_arg_for_type(ty)))
        .collect::<Vec<_>>()
        .join(",");
    let pins = core_member_pins_json(&params);
    let unavailable = core_member_unavailable_json(member);
    format!(
        "{{\"action_id\":{},\"kind\":\"canvas.core_catalog\",\"title\":{},\"module_path\":{},\"callee\":{},\"insert_callee\":{},\"insert_op\":\"insert_call\",\"engine\":\"checked-tir+jit\",\"execution\":\"source_transaction\",\"available\":{},{}\"authority\":[{}],\"package_id\":{},\"version\":{},\"touched_files\":[{}],\"writes\":\"source_transaction_only\",\"requires_confirmation\":false,\"audit\":[\"source\",\"module_path\",\"signature\",\"diff\",\"diagnostics\"],\"signature\":{},\"pure\":{},\"summary\":{},\"source\":{},\"pins\":[{}],\"default_args\":[{}]}}",
        json_str(&action_id),
        json_str(&format!("{} · {}", member.name, module_path)),
        json_str(module_path),
        json_str(&callee),
        json_str(&callee),
        if member.available { "true" } else { "false" },
        unavailable,
        json_str(&authority.grant),
        json_str(&authority.package_id),
        json_str(&authority.version),
        json_str(&authority.touched_file),
        json_str(&member.signature),
        if member.pure { "true" } else { "false" },
        json_str(&member.summary),
        json_str(&member.source),
        pins,
        default_args
    )
}

#[derive(Clone)]
struct CoreCatalogModule {
    path: String,
    title: String,
    summary: String,
    members: Vec<CoreCatalogMember>,
}

#[derive(Clone)]
struct CoreCatalogMember {
    name: String,
    signature: String,
    summary: String,
    source: String,
    pure: bool,
    available: bool,
    unavailable_reason_code: String,
    unavailable_reason: String,
}

fn core_catalog_entries(query: &str) -> Vec<CoreCatalogModule> {
    let needle = query.trim();
    let mut modules = parse_core_catalog_markdown(include_str!("../../docs/reference/core-library.md"));
    let exports = parse_sema_core_module_items(
        include_str!("../../crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs"),
    );
    merge_sema_core_registry(
        &mut modules,
        include_str!("../../crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs"),
    );
    mark_core_catalog_availability(&mut modules, &exports);
    if !needle.is_empty() {
        modules = modules
            .into_iter()
            .filter_map(|mut module| {
                module.members = module
                    .members
                    .into_iter()
                    .filter(|member| {
                        contains_ci(&member.name, needle)
                            || contains_ci(&member.signature, needle)
                            || contains_ci(&member.summary, needle)
                    })
                    .collect();
                if contains_ci(&module.path, needle)
                    || contains_ci(&module.title, needle)
                    || contains_ci(&module.summary, needle)
                    || !module.members.is_empty()
                {
                    Some(module)
                } else {
                    None
                }
            })
            .collect();
    }
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    for module in &mut modules {
        module.members.sort_by(|a, b| a.name.cmp(&b.name));
    }
    modules
}

fn merge_sema_core_registry(modules: &mut Vec<CoreCatalogModule>, registry: &str) {
    for (path, names) in parse_sema_core_module_items(registry) {
        let pos = modules
            .iter()
            .position(|module| module.path == path)
            .unwrap_or_else(|| {
                modules.push(CoreCatalogModule {
                    title: path.clone(),
                    path: path.clone(),
                    summary: "Compiler-known Core module from the sema registry.".to_string(),
                    members: Vec::new(),
                });
                modules.len() - 1
            });
        let module = &mut modules[pos];
        if module.summary.is_empty() {
            module.summary = "Compiler-known Core module from the sema registry.".to_string();
        }
        for name in names {
            if module.members.iter().any(|member| member.name == name) {
                continue;
            }
            module.members.push(CoreCatalogMember {
                signature: format!("{path}.{name}"),
                summary: "Compiler-known Core item from sema registry.".to_string(),
                source: "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs".to_string(),
                pure: core_member_pure(&path, &name),
                name,
                available: true,
                unavailable_reason_code: String::new(),
                unavailable_reason: String::new(),
            });
        }
    }
}

fn mark_core_catalog_availability(
    modules: &mut [CoreCatalogModule],
    exports: &[(String, Vec<String>)],
) {
    for module in modules {
        for member in &mut module.members {
            if core_export_resolves(exports, &module.path, &member.name) {
                if let Some((code, reason)) = core_member_direct_exclusion(&module.path, member) {
                    member.available = false;
                    member.unavailable_reason_code = code.to_string();
                    member.unavailable_reason = reason;
                } else {
                    member.available = true;
                    member.unavailable_reason_code.clear();
                    member.unavailable_reason.clear();
                }
            } else {
                let (code, reason) = core_member_unavailable_reason(&module.path, member);
                member.available = false;
                member.unavailable_reason_code = code.to_string();
                member.unavailable_reason = reason;
            }
        }
    }
}

fn core_member_direct_exclusion(
    module_path: &str,
    member: &CoreCatalogMember,
) -> Option<(&'static str, String)> {
    if member
        .name
        .chars()
        .next()
        .map_or(false, |c| c.is_ascii_uppercase())
    {
        return Some((
            "type_only",
            "Use this as a type or constructor surface, not a direct call.".to_string(),
        ));
    }
    if module_path == "core.crypto.expert" {
        return Some((
            "needs_unsafe_region",
            "Needs an #Unsafe region before Canvas can insert it.".to_string(),
        ));
    }
    if !core_module_has_canvas_defaults(module_path) {
        return Some((
            "needs_canvas_defaults",
            "Needs Canvas argument defaults before this entry can be inserted.".to_string(),
        ));
    }
    if module_path.starts_with("core.encoding.") && !core_encoding_member_has_canvas_defaults(module_path, &member.name) {
        return Some((
            "needs_canvas_defaults",
            "Needs Canvas argument defaults before this entry can be inserted.".to_string(),
        ));
    }
    let params = core_member_params(module_path, &member.name, &member.signature);
    let generated_signature = member.signature == format!("{module_path}.{}", member.name);
    if generated_signature && params.is_empty() {
        return Some((
            "needs_signature",
            "Needs parameter details before Canvas can insert it.".to_string(),
        ));
    }
    if !member.signature.contains('(') && params.is_empty() {
        return Some((
            "value_only",
            "Use this as a value, not a direct call.".to_string(),
        ));
    }
    None
}

fn core_module_has_canvas_defaults(module_path: &str) -> bool {
    matches!(
        module_path,
        "core.math"
            | "core.encoding.json"
            | "core.encoding.jsonl"
            | "core.encoding.csv"
            | "core.encoding.toml"
            | "core.encoding.yaml"
            | "core.encoding.xml"
            | "core.encoding.cbor"
            | "core.encoding.hex"
            | "core.encoding.base64"
            | "core.encoding.base32"
    )
}

fn core_encoding_member_has_canvas_defaults(module_path: &str, member: &str) -> bool {
    matches!(
        (module_path, member),
        ("core.encoding.json", "parse" | "decode")
            | (
                "core.encoding.hex" | "core.encoding.base64" | "core.encoding.base32",
                "decode" | "decode_url"
            )
    )
}

fn core_export_resolves(exports: &[(String, Vec<String>)], module_path: &str, member: &str) -> bool {
    exports
        .iter()
        .any(|(path, names)| path == module_path && names.iter().any(|name| name == member))
}

fn core_member_unavailable_reason(
    module_path: &str,
    member: &CoreCatalogMember,
) -> (&'static str, String) {
    let sig = member.signature.trim();
    if module_path == "core.args" && sig.starts_with('.') {
        return (
            "method_only",
            "Use this as a method on an ArgsSpec value.".to_string(),
        );
    }
    if sig.starts_with('.') || sig.contains(").") || sig.contains("::") {
        return (
            "method_only",
            "Use this from the value shown in the signature.".to_string(),
        );
    }
    if sig.contains('.') && !sig.starts_with(module_path) {
        return (
            "type_member",
            "Use this from the type shown in the signature.".to_string(),
        );
    }
    (
        "not_direct_call",
        "This Core entry is documentation only; it has no direct module call.".to_string(),
    )
}

fn parse_sema_core_module_items(registry: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut lines = registry.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if !trimmed.starts_with('"') || !trimmed.contains("=> &[") {
            continue;
        }
        let Some(path) = parse_quoted(trimmed) else {
            continue;
        };
        if !(path == "core" || path.starts_with("core.") || path.starts_with("jet.")) {
            continue;
        }
        let path = user_core_module_path(&path);
        let mut names = Vec::new();
        let after_array = trimmed
            .split_once("&[")
            .map(|(_, rest)| rest)
            .unwrap_or_default();
        if let Some((same_line, _)) = after_array.split_once(']') {
            for name in quoted_strings(same_line) {
                if !names.iter().any(|existing| existing == &name) {
                    names.push(name);
                }
            }
            out.push((path, names));
            continue;
        }
        for body_line in lines.by_ref() {
            let body = body_line.trim();
            if body.starts_with("],") || body.starts_with("]") {
                break;
            }
            for name in quoted_strings(body) {
                if !names.iter().any(|existing| existing == &name) {
                    names.push(name);
                }
            }
        }
        out.push((path, names));
    }
    out
}

fn user_core_module_path(path: &str) -> String {
    if let Some(ring) = path.strip_prefix("jet.") {
        format!("core.{ring}")
    } else {
        path.to_string()
    }
}

fn quoted_strings(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(value) = parse_quoted(rest) {
        let skip = rest.find('"').map(|i| i + 1).unwrap_or(0);
        let after_open = &rest[skip..];
        let Some(end) = after_open.find('"') else {
            break;
        };
        out.push(value);
        rest = &after_open[end + 1..];
    }
    out
}

fn parse_quoted(text: &str) -> Option<String> {
    let first = text.find('"')?;
    let rest = &text[first + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_core_catalog_markdown(markdown: &str) -> Vec<CoreCatalogModule> {
    let mut modules = Vec::new();
    let mut current: Option<CoreCatalogModule> = None;
    for line in markdown.lines() {
        if let Some(path) = core_heading_path(line) {
            if let Some(module) = current.take() {
                modules.push(module);
            }
            current = Some(CoreCatalogModule {
                path,
                title: strip_markdown_heading(line),
                summary: String::new(),
                members: Vec::new(),
            });
            continue;
        }
        let Some(module) = current.as_mut() else {
            continue;
        };
        let trimmed = line.trim();
        if module.summary.is_empty()
            && !trimmed.is_empty()
            && !trimmed.starts_with('|')
            && !trimmed.starts_with("```")
            && !trimmed.starts_with('#')
        {
            module.summary = strip_markdown_inline(trimmed);
        }
        if let Some(member) = core_catalog_member_from_line(trimmed) {
            if !module
                .members
                .iter()
                .any(|existing| existing.signature == member.signature)
            {
                module.members.push(member);
            }
        }
    }
    if let Some(module) = current {
        modules.push(module);
    }
    for path in core_paths_in_markdown(markdown) {
        if !modules.iter().any(|module| module.path == path) {
            modules.push(CoreCatalogModule {
                title: path.clone(),
                path,
                summary: "Listed in the canonical Core library reference.".to_string(),
                members: Vec::new(),
            });
        }
    }
    modules
}

fn core_paths_in_markdown(markdown: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in markdown.split(|c: char| {
        !(c.is_ascii_alphanumeric() || c == '_' || c == '.')
    }) {
        if token == "core" || token.starts_with("core.") {
            let path = token.trim_matches('.');
            if !path.is_empty() && !paths.iter().any(|existing| existing == path) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

fn core_heading_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("## ") || trimmed.starts_with("### ")) {
        return None;
    }
    let first = trimmed.find('`')?;
    let rest = &trimmed[first + 1..];
    let end = rest.find('`')?;
    let path = &rest[..end];
    if path == "core" || path.starts_with("core.") {
        Some(path.to_string())
    } else {
        None
    }
}

fn core_catalog_member_from_line(line: &str) -> Option<CoreCatalogMember> {
    if !line.starts_with('|') || !line.contains('`') || line.starts_with("| ---") {
        return None;
    }
    let first = line.find('`')?;
    let rest = &line[first + 1..];
    let end = rest.find('`')?;
    let signature = rest[..end].trim();
    if signature.is_empty() || signature == "Function/method" || signature == "Type" {
        return None;
    }
    let name_source = signature
        .split('/')
        .next()
        .unwrap_or(signature)
        .split('(')
        .next()
        .unwrap_or(signature)
        .trim();
    let name = name_source
        .rsplit('.')
        .next()
        .unwrap_or(name_source)
        .split_whitespace()
        .next()
        .unwrap_or(name_source)
        .trim_matches('`')
        .to_string();
    let summary = line
        .split('|')
        .nth(3)
        .map(strip_markdown_inline)
        .unwrap_or_default();
    Some(CoreCatalogMember {
        name,
        signature: signature.to_string(),
        summary,
        source: "docs/reference/core-library.md".to_string(),
        pure: core_member_pure_for_signature(signature),
        available: true,
        unavailable_reason_code: String::new(),
        unavailable_reason: String::new(),
    })
}

fn core_source_callee(src: &str, module_path: &str) -> String {
    if let Some(alias) = imported_core_alias(src, module_path) {
        alias
    } else {
        module_path.to_string()
    }
}

fn imported_core_alias(src: &str, module_path: &str) -> Option<String> {
    let prefix = format!("use {module_path} as ");
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or("").trim();
        let Some(rest) = code.strip_prefix(&prefix) else {
            continue;
        };
        let alias = rest
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("")
            .trim();
        if !alias.is_empty() {
            return Some(alias.to_string());
        }
    }
    None
}

fn core_member_pure_for_signature(signature: &str) -> bool {
    let head = signature
        .split('/')
        .next()
        .unwrap_or(signature)
        .split('(')
        .next()
        .unwrap_or(signature)
        .trim();
    let parts = head.split('.').collect::<Vec<_>>();
    if parts.len() >= 2 {
        let name = parts.last().copied().unwrap_or(head);
        let module = parts[..parts.len() - 1].join(".");
        return core_member_pure(&format!("core.{module}"), name);
    }
    true
}

fn core_member_pure(module_path: &str, name: &str) -> bool {
    if matches!(
        module_path,
        "core.math"
            | "core.fmt"
            | "core.text"
            | "core.text.unicode"
            | "core.encoding"
            | "core.encoding.json"
            | "core.encoding.jsonl"
            | "core.encoding.csv"
            | "core.encoding.toml"
            | "core.encoding.yaml"
            | "core.encoding.xml"
            | "core.encoding.cbor"
            | "core.encoding.hex"
            | "core.encoding.base64"
            | "core.encoding.base32"
            | "core.url"
            | "core.mime"
            | "core.reflect"
            | "core.solve"
            | "core.data"
    ) {
        return true;
    }
    if module_path == "core.files" {
        return matches!(name, "exists" | "is_dir" | "absolute");
    }
    if module_path == "core.env" {
        return matches!(name, "get" | "current_dir" | "home_dir");
    }
    if module_path == "core.os" {
        return name != "set_current_dir" && name != "on_interrupt";
    }
    if module_path == "core.time" {
        return matches!(name, "ms" | "secs" | "seconds" | "minutes" | "hours" | "from_unix_ms" | "period" | "period_days" | "period_months" | "period_years" | "zone" | "utc" | "zoned");
    }
    false
}

fn project_function_catalog_json(
    def: &jet_semindex::SymbolDef,
    params: &[(String, String)],
    ret: Option<&str>,
    index: &SemIndex,
) -> String {
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
        "{{\"name\":{},\"signature\":{},\"callee\":{},\"module_path\":{},\"pure\":{},\"ret\":{},\"pins\":[{}],\"default_args\":[{}],\"available\":true,\"source_span\":{},\"insert_op\":\"insert_call\"}}",
        json_str(&def.name),
        json_str(&function_signature_from_parts(&def.name, params, ret)),
        json_str(&def.name),
        json_str(&def.module_path),
        if call_has_effects(index, &def.name) { "false" } else { "true" },
        json_str(ret.unwrap_or("Void")),
        pins,
        default_args,
        span_json(def.def_span)
    )
}

fn function_signature_from_parts(
    name: &str,
    params: &[(String, String)],
    ret: Option<&str>,
) -> String {
    let params = params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    let ret = ret
        .map(|ty| format!(" -> {ty}"))
        .unwrap_or_default();
    format!("fn {name}({params}){ret}")
}

fn core_module_json(module: &CoreCatalogModule) -> String {
    let members = module
        .members
        .iter()
        .map(|member| core_member_json(&module.path, member))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"path\":{},\"title\":{},\"summary\":{},\"source\":{},\"members\":[{}]}}",
        json_str(&module.path),
        json_str(&module.title),
        json_str(&module.summary),
        json_str("docs/reference/core-library.md"),
        members
    )
}

fn core_member_json(module_path: &str, member: &CoreCatalogMember) -> String {
    let params = core_member_params(module_path, &member.name, &member.signature);
    let default_args = params
        .iter()
        .map(|(_, ty)| json_str(&default_arg_for_type(ty)))
        .collect::<Vec<_>>()
        .join(",");
    let pins = core_member_pins_json(&params);
    let unavailable = core_member_unavailable_json(member);
    format!(
        "{{\"name\":{},\"signature\":{},\"pure\":{},\"summary\":{},\"source\":{},\"writes\":\"none\",\"available\":{},{}\"pins\":[{}],\"default_args\":[{}]}}",
        json_str(&member.name),
        json_str(&member.signature),
        if member.pure { "true" } else { "false" },
        json_str(&member.summary),
        json_str(&member.source),
        if member.available { "true" } else { "false" },
        unavailable,
        pins,
        default_args
    )
}

fn core_member_unavailable_json(member: &CoreCatalogMember) -> String {
    if member.available {
        String::new()
    } else {
        format!(
            "\"unavailable_reason_code\":{},\"denied_reason\":{},",
            json_str(&member.unavailable_reason_code),
            json_str(&member.unavailable_reason)
        )
    }
}

fn core_member_pins_json(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(name, ty)| {
            format!(
                "{{\"name\":{},\"direction\":\"input\",\"type\":{}}}",
                json_str(name),
                json_str(ty)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn strip_markdown_heading(line: &str) -> String {
    strip_markdown_inline(line.trim_start_matches('#').trim())
}

fn strip_markdown_inline(text: &str) -> String {
    text.replace('`', "")
        .replace("**", "")
        .replace("<br>", " ")
        .trim()
        .to_string()
}

fn canvas_action_json(
    def: &jet_semindex::SymbolDef,
    params: &[(String, String)],
    ret: Option<&str>,
    authority: &CanvasAuthority,
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
        "{{\"action_id\":{},\"kind\":\"canvas.action\",\"title\":{},\"callee\":{},\"module_path\":{},\"engine\":\"checked-tir+jit\",\"authority\":[{}],\"package_id\":{},\"version\":{},\"touched_files\":[{}],\"writes\":\"source_transaction_only\",\"audit\":[\"package_id\",\"version\",\"hash\",\"authority\",\"touched_files\",\"diff\",\"diagnostics\"],\"source_span\":{},\"ret\":{},\"pins\":[{}],\"default_args\":[{}]}}",
        json_str(&action_id),
        json_str(&def.name),
        json_str(&def.name),
        json_str(&def.module_path),
        json_str(&authority.grant),
        json_str(&authority.package_id),
        json_str(&authority.version),
        json_str(&authority.touched_file),
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
    authority: &CanvasAuthority,
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
        "{{\"action_id\":{},\"kind\":\"canvas.builtin\",\"title\":{},\"callee\":{},\"module_path\":\"builtin\",\"engine\":\"checked-tir+jit\",\"authority\":[{}],\"package_id\":{},\"version\":{},\"touched_files\":[{}],\"writes\":\"source_transaction_only\",\"audit\":[\"package_id\",\"version\",\"hash\",\"authority\",\"touched_files\",\"diff\",\"diagnostics\"],\"source_span\":null,\"ret\":{},\"pins\":[{}],\"default_args\":[{}]}}",
        json_str(&action_id),
        json_str(title),
        json_str(callee),
        json_str(&authority.grant),
        json_str(&authority.package_id),
        json_str(&authority.version),
        json_str(&authority.touched_file),
        json_str(ret),
        pins,
        default_args
    )
}

fn core_member_params(module_path: &str, member_name: &str, signature: &str) -> Vec<(String, String)> {
    if let Some(params) = params_from_signature(signature) {
        return params;
    }
    match (module_path, member_name) {
        ("core.math", "abs") => {
            vec![("value".to_string(), "Int".to_string())]
        }
        (
            "core.math",
            "sqrt" | "floor" | "ceil" | "round" | "sin" | "cos" | "tan" | "asin" | "acos"
            | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "ln" | "log2" | "log10"
            | "trunc" | "fract" | "sign" | "degrees" | "radians",
        ) => {
            vec![("value".to_string(), "Float".to_string())]
        }
        ("core.math", "int_pow" | "gcd" | "lcm") => vec![
            ("left".to_string(), "Int".to_string()),
            ("right".to_string(), "Int".to_string()),
        ],
        ("core.math", "pow" | "atan2" | "hypot") => vec![
            ("left".to_string(), "Float".to_string()),
            ("right".to_string(), "Float".to_string()),
        ],
        ("core.math", "min" | "max") => vec![
            ("left".to_string(), "Int".to_string()),
            ("right".to_string(), "Int".to_string()),
        ],
        ("core.math", "clamp") => vec![
            ("value".to_string(), "Int".to_string()),
            ("min".to_string(), "Int".to_string()),
            ("max".to_string(), "Int".to_string()),
        ],
        ("core.encoding.json" | "core.encoding.csv" | "core.encoding.toml" | "core.encoding.yaml", "parse" | "decode" | "decode_traced") => {
            vec![("text".to_string(), "String".to_string())]
        }
        ("core.encoding.hex" | "core.encoding.base64" | "core.encoding.base32", "decode" | "decode_url") => {
            vec![("text".to_string(), "String".to_string())]
        }
        _ => Vec::new(),
    }
}

fn params_from_signature(signature: &str) -> Option<Vec<(String, String)>> {
    let start = signature.find('(')?;
    let end = signature[start + 1..].find(')')? + start + 1;
    let body = signature[start + 1..end].trim();
    if body.is_empty() {
        return Some(Vec::new());
    }
    let mut params = Vec::new();
    for (i, raw) in body.split(',').enumerate() {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        let (name, ty) = if let Some((name, ty)) = part.split_once(':') {
            (name.trim(), normalize_param_type(ty.trim()))
        } else {
            let name = part
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("")
                .trim();
            (name, type_for_param_name(name))
        };
        let name = if name.is_empty() {
            format!("arg{}", i + 1)
        } else {
            name.to_string()
        };
        params.push((name, ty));
    }
    Some(params)
}

fn normalize_param_type(ty: &str) -> String {
    ty.split('=')
        .next()
        .unwrap_or(ty)
        .trim()
        .trim_end_matches('?')
        .trim()
        .to_string()
}

fn type_for_param_name(name: &str) -> String {
    match name {
        "text" | "raw" | "body" | "path" | "name" | "url" | "host" | "scheme" | "query"
        | "fragment" | "delim" | "method" => "String",
        "ok" | "enabled" | "flag" => "Bool",
        _ => "Int",
    }
    .to_string()
}

fn default_arg_for_type(ty: &str) -> String {
    let ty = ty.trim().trim_end_matches('?').trim();
    match ty {
        "Bool" => "true".to_string(),
        "String" | "Path" | "Url" => "\"canvas\"".to_string(),
        "Float" | "F32" | "F64" | "Decimal" => "1.0".to_string(),
        _ => "1".to_string(),
    }
}

fn canvas_command_action_jsons(authority: &CanvasAuthority) -> Vec<String> {
    let source = authority.touched_file.as_str();
    let mut actions = vec![
        canvas_command_action_json(
            "run",
            "Run program",
            &["jet", "run", source],
            "none",
            &["canvas.command:run", authority.grant.as_str()],
            true,
            None,
            authority,
        ),
        canvas_command_action_json(
            "check",
            "Check project",
            &["jet", "check", source],
            "none",
            &[
                "canvas.command:check",
                authority.grant.as_str(),
            ],
            true,
            None,
            authority,
        ),
        canvas_command_action_json(
            "test",
            "Test project",
            &["jet", "test", source],
            "test_outputs",
            &[
                "canvas.command:test",
                "canvas.build_output:test",
                authority.grant.as_str(),
            ],
            true,
            None,
            authority,
        ),
        canvas_command_action_json(
            "build",
            "Build project",
            &["jet", "build", source],
            "build_outputs",
            &[
                "canvas.command:build",
                "canvas.build_output:binary",
                authority.grant.as_str(),
            ],
            true,
            None,
            authority,
        ),
        canvas_command_action_json(
            "dev",
            "Run dev server",
            &["jet", "dev", source, "--target=web"],
            "dev_server",
            &[
                "canvas.command:dev",
                "canvas.service:dev_server",
                authority.grant.as_str(),
            ],
            true,
            None,
            authority,
        ),
    ];
    let services_available = !env_project_json(&authority.project_root).services.is_empty();
    actions.push(canvas_command_action_json(
        "service.start",
        "Start service",
        &["jetpack", "dev", "service", "start"],
        "service_process",
        &["canvas.service:start", "canvas.env:service"],
        services_available,
        if services_available {
            None
        } else {
            Some("no env service selected")
        },
        authority,
    ));
    actions
}

fn canvas_command_action_json(
    name: &str,
    title: &str,
    command: &[&str],
    writes: &str,
    grants: &[&str],
    available: bool,
    denied: Option<&str>,
    authority: &CanvasAuthority,
) -> String {
    let command_json = command
        .iter()
        .map(|arg| json_str(arg))
        .collect::<Vec<_>>()
        .join(",");
    let grants_json = grants
        .iter()
        .map(|grant| json_str(grant))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"action_id\":{},\"kind\":\"canvas.command\",\"title\":{},\"op\":\"command_authority\",\"engine\":\"jet-cli\",\"execution\":\"external_command\",\"available\":{},\"denied_reason\":{},\"command\":[{}],\"authority\":[{}],\"package_id\":{},\"version\":{},\"touched_files\":[{}],\"writes\":{},\"requires_confirmation\":{},\"audit\":[\"package_id\",\"version\",\"command\",\"authority\",\"touched_files\",\"diagnostics\"]}}",
        json_str(&format!("canvas.command:{name}")),
        json_str(title),
        if available { "true" } else { "false" },
        json_optional_str(denied),
        command_json,
        grants_json,
        json_str(&authority.package_id),
        json_str(&authority.version),
        json_str(&authority.touched_file),
        json_str(writes),
        if writes == "none" { "false" } else { "true" },
    )
}

struct CanvasAuthority {
    grant: String,
    package_id: String,
    version: String,
    touched_file: String,
    project_root: PathBuf,
}

fn canvas_authority_context(path: &Path) -> CanvasAuthority {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    if let Some(root) = crate::Loader::find_manifest_root(dir) {
        let manifest_path = root.join(crate::Syntax::PAYLOAD_FILE);
        if let Ok(raw) = fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = crate::Jetpack::PackageManifest::parse(&raw) {
                return CanvasAuthority {
                    grant: "canvas.source_edit:package".to_string(),
                    package_id: manifest.package.name,
                    version: manifest.package.version,
                    touched_file: rel_path(&root, path),
                    project_root: root,
                };
            }
        }
    }
    CanvasAuthority {
        grant: "canvas.source_edit:single_file".to_string(),
        package_id: "single-file".to_string(),
        version: "unpackaged".to_string(),
        touched_file: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("current.jet")
            .to_string(),
        project_root: dir.to_path_buf(),
    }
}
