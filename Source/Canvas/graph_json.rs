fn add_node(
    g: &mut GraphBuilder,
    id: &str,
    kind: &str,
    archetype: &str,
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
        archetype: archetype.to_string(),
        title: title.to_string(),
        span,
        x,
        y,
        badges: badges.into_iter().map(str::to_string).collect(),
        affordances: affordances.into_iter().map(str::to_string).collect(),
        meta_json: None,
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
        role: None,
        pattern_source: None,
        capability: capability.to_string(),
        fallible,
        effect_grant_need: None,
        span,
    });
    id
}

fn add_arm_pin(g: &mut GraphBuilder, node_id: &str, name: &str, pattern_source: &str) -> String {
    let id = format!("{node_id}:output:{name}");
    let span = g
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.span)
        .unwrap_or(SourceSpan { start: 0, end: 0 });
    if !g.pins.iter().any(|pin| pin.id == id) {
        g.pins.push(PinRec {
            id: id.clone(),
            node_id: node_id.to_string(),
            name: name.to_string(),
            direction: "output".to_string(),
            ty: "exec".to_string(),
            role: Some("arm".to_string()),
            pattern_source: Some(pattern_source.to_string()),
            capability: "control".to_string(),
            fallible: false,
            effect_grant_need: None,
            span,
        });
    }
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
        from_span: None,
        to_span: None,
    });
}

fn add_control_wire(
    g: &mut GraphBuilder,
    from_pin: &str,
    to_pin: &str,
    from_span: SourceSpan,
    to_span: SourceSpan,
) {
    g.next_wire += 1;
    g.wires.push(WireRec {
        id: format!("{}:wire:{}", g.graph_id, g.next_wire),
        from_pin: from_pin.to_string(),
        to_pin: to_pin.to_string(),
        kind: "control".to_string(),
        span: Some(to_span),
        from_span: Some(from_span),
        to_span: Some(to_span),
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

fn meta_attr_json(meta: Option<&AST::MetaAttr>) -> Option<String> {
    let meta = meta?;
    let facts = meta.facts();
    if facts.category.is_none() && !facts.tunable {
        return None;
    }
    let category = facts
        .category
        .as_ref()
        .map(|s| json_str(s))
        .unwrap_or_else(|| "null".to_string());
    Some(format!(
        "{{\"category\":{},\"tunable\":{}}}",
        category,
        if facts.tunable { "true" } else { "false" }
    ))
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
        if node_has_arm_pins(g, &node.id) {
            ensure_exec_pin(g, &node.id, "else", "output", node.span);
        } else if node.kind == "branch" || (node.kind == "function" && node.archetype == "control") {
            ensure_exec_pin(g, &node.id, "then", "output", node.span);
            ensure_exec_pin(g, &node.id, "else", "output", node.span);
        } else if node.kind != "return" {
            ensure_exec_pin(g, &node.id, "then", "output", node.span);
        }
    }

    let mut previous_out: Option<(String, SourceSpan)> = None;
    for node in &exec_nodes {
        let input = format!("{}:input:exec", node.id);
        if let Some((from, from_span)) = previous_out.take() {
            if node.kind != "entry" {
                add_control_wire(g, &from, &input, from_span, node.span);
            }
        }
        previous_out = if node.kind == "return" {
            None
        } else {
            Some((primary_exec_output(g, &node.id), node.span))
        };
    }
}

fn node_has_arm_pins(g: &GraphBuilder, node_id: &str) -> bool {
    g.pins.iter().any(|pin| {
        pin.node_id == node_id
            && pin.direction == "output"
            && pin.role.as_deref() == Some("arm")
    })
}

fn primary_exec_output(g: &GraphBuilder, node_id: &str) -> String {
    g.pins
        .iter()
        .find(|pin| {
            pin.node_id == node_id
                && pin.direction == "output"
                && pin.ty == "exec"
                && pin.role.as_deref() == Some("arm")
        })
        .map(|pin| pin.id.clone())
        .unwrap_or_else(|| format!("{node_id}:output:then"))
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
    matches!(node.archetype.as_str(), "entry" | "function_exec" | "control")
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
            role: None,
            pattern_source: None,
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
        "{{\"name\":{},\"signature\":{},\"visibility\":{},\"docs\":{},\"pure\":{},\"unsafe\":{},\"effects\":[{}],\"returns\":{},\"params\":[{}],\"meta\":{},\"source_span\":{},\"edit_affordances\":[\"rename_function\",\"edit_function_signature\",\"create_function\",\"source_jump\"]}}",
        json_str(&f.name),
        json_str(&function_signature_text(src, f)),
        json_str(function_visibility(f)),
        json_str(&doc_comment_before(src, f.name_span.start)),
        if f.is_pure { "true" } else { "false" },
        if f.is_unsafe { "true" } else { "false" },
        effects,
        json_str(&ret),
        params,
        meta_attr_json(f.meta.as_ref()).unwrap_or_else(|| "null".to_string()),
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
    let meta = n
        .meta_json
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"node_id\":{},\"kind\":{},\"archetype\":{},\"title\":{},\"source_span\":{},\"layout\":{{\"x\":{},\"y\":{}}},\"badges\":[{}],\"edit_affordances\":[{}],\"meta\":{}}}",
        json_str(&n.id),
        json_str(&n.kind),
        json_str(&n.archetype),
        json_str(&n.title),
        span_json(n.span),
        n.x,
        n.y,
        json_strs(&n.badges),
        json_strs(&n.affordances),
        meta
    )
}

fn pin_json(p: &PinRec) -> String {
    let grant = p
        .effect_grant_need
        .as_ref()
        .map(|s| json_str(s))
        .unwrap_or_else(|| "null".to_string());
    let role = p
        .role
        .as_ref()
        .map(|s| json_str(s))
        .unwrap_or_else(|| "null".to_string());
    let pattern_source = p
        .pattern_source
        .as_ref()
        .map(|s| json_str(s))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"pin_id\":{},\"node_id\":{},\"name\":{},\"direction\":{},\"type\":{},\"role\":{},\"pattern_source\":{},\"capability\":{},\"fallible\":{},\"effect_grant_need\":{},\"source_span\":{}}}",
        json_str(&p.id),
        json_str(&p.node_id),
        json_str(&p.name),
        json_str(&p.direction),
        json_str(&p.ty),
        role,
        pattern_source,
        json_str(&p.capability),
        if p.fallible { "true" } else { "false" },
        grant,
        span_json(p.span)
    )
}

fn wire_json(w: &WireRec) -> String {
    let source_span = w.span.map(span_json).unwrap_or_else(|| "null".to_string());
    let from_span = w
        .from_span
        .map(span_json)
        .unwrap_or_else(|| "null".to_string());
    let to_span = w
        .to_span
        .map(span_json)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"wire_id\":{},\"from_pin\":{},\"to_pin\":{},\"wire_kind\":{},\"source_span\":{},\"from_source_span\":{},\"to_source_span\":{}}}",
        json_str(&w.id),
        json_str(&w.from_pin),
        json_str(&w.to_pin),
        json_str(&w.kind),
        source_span,
        from_span,
        to_span
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
