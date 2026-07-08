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
    for def in index.definitions() {
        let SymbolKind::Function { params, ret } = &def.kind else {
            continue;
        };
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
    entries.extend(canvas_command_action_jsons(&authority));
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

fn default_arg_for_type(ty: &str) -> String {
    match ty {
        "Bool" => "true".to_string(),
        "String" => "\"canvas\"".to_string(),
        "Float" | "F32" | "F64" => "1.0".to_string(),
        _ => "1".to_string(),
    }
}

fn canvas_command_action_jsons(authority: &CanvasAuthority) -> Vec<String> {
    let source = authority.touched_file.as_str();
    let mut actions = vec![
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
