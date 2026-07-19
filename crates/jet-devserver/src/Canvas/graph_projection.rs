use std::path::Path;

use jet_driver::AST::{self, Expr, Item, Stmt};
use jet_driver::Diagnostics::Span;
use jet_semindex::{SemIndex, SemIndexEffectFacts, SourceSpan, SymbolKind};

use super::graph_helpers::{
    assignment_title, binding_type, call_has_effects, call_ret, effect_badges, expr_title,
    expr_type, graph_id, insert_offset, lvalue_type, pure_leaf, snippet, starts_uppercase,
    text_matches, wire_ident_refs,
};
use super::graph_json::{
    add_arm_pin, add_execution_overlay, add_inline, add_node, add_pin, add_region,
    add_source_comment_regions, add_wire, add_wire_with_span, graph_to_json, meta_attr_json,
    set_pin_append, set_pin_source_span,
};
use super::schema_api::{
    GRAPH_SCHEMA_VERSION, GraphBuilder, GraphEditAnchor, InlineExpr, NodeQueryRef, NodeRec,
    PinRec, Projection, source_revision,
};
use super::validation_json::{json_str, span_json};

pub(super) fn project_checked(
    path: &Path,
    src: &str,
    bundle: &AST::ProgramBundle,
    facts: &SemIndexEffectFacts,
    runtime_events: Option<&str>,
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
    let fmt = jet_driver::Formatter::format_source(src).unwrap_or_else(|_| src.to_string());
    let blueprint = canvas_blueprint_facts_json(src, bundle, &index, runtime_events);
    let json = format!(
        "{{\"protocol\":\"jet.canvas.graph\",\"schema_version\":{},\"source_id\":{},\"revision\":{},\"fmt_fingerprint\":{},\"source_text\":{},\"graphs\":[{}],\"diagnostics\":[],\"facts\":{{\"semindex_schema_version\":{},\"handles\":[\"definitions\",\"references\",\"calls\",\"effects\",\"members\",\"outputs\"],\"blueprint\":{}}}}}",
        GRAPH_SCHEMA_VERSION,
        json_str(&path.display().to_string()),
        json_str(&source_revision(src)),
        json_str(&source_revision(&fmt)),
        json_str(src),
        graph_json.join(","),
        index.schema_version(),
        blueprint
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
                    fallible: function_is_fallible(f),
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
                        fallible: function_is_fallible(method),
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
                        fallible: function_is_fallible(method),
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

fn function_is_fallible(f: &AST::Func) -> bool {
    matches!(f.return_type.as_ref(), Some(AST::Type::Result { .. }))
}

fn canvas_blueprint_facts_json(
    src: &str,
    bundle: &AST::ProgramBundle,
    index: &SemIndex,
    runtime_events: Option<&str>,
) -> String {
    let mut interfaces = Vec::new();
    collect_interface_facts(
        &bundle
            .modules
            .iter()
            .flat_map(|m| m.items.iter())
            .collect::<Vec<_>>(),
        &mut interfaces,
    );
    let task_flows = task_flow_facts(src).join(",");
    let outputs = index.outputs().iter().map(output_fact_json).collect::<Vec<_>>().join(",");
    format!(
        "{{\"runtime_events\":{},\"interfaces\":[{}],\"task_flows\":[{}],\"outputs\":[{}],\"source_truth\":\"ordinary_jet_source\"}}",
        runtime_events.unwrap_or("null"),
        interfaces.join(","),
        task_flows,
        outputs,
    )
}

fn output_fact_json(output: &jet_semindex::OutputFact) -> String {
    let effects = output.entry.effects.iter().map(|effect| json_str(effect)).collect::<Vec<_>>().join(",");
    format!(
        "{{\"binding\":{},\"kind\":{},\"name\":{},\"entry\":{{\"identity\":{},\"name\":{},\"module_path\":{},\"definition_span\":{},\"reference_span\":{},\"effects\":[{}]}},\"fact_source\":\"semindex_resolved_output\"}}",
        json_str(&output.binding), json_str(&output.kind), json_str(&output.name),
        json_str(&output.entry.identity), json_str(&output.entry.name),
        json_str(&output.entry.module_path), span_json(output.entry.definition_span),
        span_json(output.entry.reference_span), effects,
    )
}

fn collect_interface_facts(items: &[&Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Trait(t) => out.push(trait_fact_json(t)),
            Item::Impl(i) if i.trait_name.is_some() => out.push(impl_fact_json(i)),
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    out.push(inline_trait_impl_fact_json(&s.name, block));
                }
            }
            Item::Enum(e) => {
                for block in &e.trait_impls {
                    out.push(inline_trait_impl_fact_json(&e.name, block));
                }
            }
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    let nested = body.iter().collect::<Vec<_>>();
                    collect_interface_facts(&nested, out);
                }
            }
            _ => {}
        }
    }
}

fn trait_fact_json(t: &AST::TraitDef) -> String {
    let methods = t
        .methods
        .iter()
        .map(|m| {
            format!(
                "{{\"name\":{},\"signature\":{},\"source_span\":{}}}",
                json_str(&m.name),
                json_str(&trait_method_signature(m)),
                span_json(m.span.into())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"trait_interface\",\"trait\":{},\"methods\":[{}],\"source_span\":{},\"authoring\":[\"create_trait_impl\",\"jump_trait_to_impls\",\"palette_trait_methods\"]}}",
        json_str(&t.name),
        methods,
        span_json(t.name_span.into())
    )
}

fn impl_fact_json(i: &AST::ImplDef) -> String {
    let methods = i
        .methods
        .iter()
        .map(|m| json_str(&m.name))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"trait_impl\",\"type\":{},\"trait\":{},\"methods\":[{}],\"delegation_field\":{},\"source_span\":{},\"diagnostic_affordance\":\"surface_missing_trait_members\"}}",
        json_str(&i.type_name),
        json_str(i.trait_name.as_deref().unwrap_or("")),
        methods,
        i.delegation_field
            .as_deref()
            .map(json_str)
            .unwrap_or_else(|| "null".to_string()),
        span_json(i.type_span.into())
    )
}

fn inline_trait_impl_fact_json(type_name: &str, block: &AST::TraitImplBlock) -> String {
    let methods = block
        .methods
        .iter()
        .map(|m| json_str(&m.name))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"trait_impl\",\"type\":{},\"trait\":{},\"methods\":[{}],\"delegation_field\":null,\"source_span\":{},\"diagnostic_affordance\":\"surface_missing_trait_members\"}}",
        json_str(type_name),
        json_str(&block.trait_name),
        methods,
        span_json(block.trait_span.into())
    )
}

pub(super) fn trait_method_signature(m: &AST::TraitMethodSig) -> String {
    let params = m
        .params
        .iter()
        .map(|p| {
            if p.name == "self" {
                format!("{}self", p.convention.sigil())
            } else {
                format!("{}{}: {}", p.convention.sigil(), p.name, p.ty.name())
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = m
        .return_type
        .as_ref()
        .map(|t| format!(" -> {}", t.name()))
        .unwrap_or_default();
    format!("fn {}({}){}", m.name, params, ret)
}

fn task_flow_facts(src: &str) -> Vec<String> {
    let mut facts = Vec::new();
    for (needle, kind) in [
        ("taskgroup", "structured_task_scope"),
        ("tasks.spawn", "spawn_task"),
        (".task(", "taskgroup_spawn"),
        (".join(", "join_task"),
        ("tasks.channel", "channel_create"),
        (".send(", "channel_send"),
        (".receive(", "channel_receive"),
        (".all(", "taskgroup_join_all"),
        ("@Context", "deadline_context"),
    ] {
        for span in text_matches(src, needle) {
            facts.push(format!(
                "{{\"kind\":{},\"source\":{},\"source_span\":{},\"rail\":\"async\",\"semantics\":\"core.tasks_source_truth\"}}",
                json_str(kind),
                json_str(needle),
                span_json(span)
            ));
        }
    }
    facts
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
        archetype: "entry".to_string(),
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
        meta_json: meta_attr_json(f.meta.as_ref()),
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
            role: None,
            pattern_source: None,
            capability: p.convention.sigil().to_string(),
            fallible: false,
            effect_grant_need: None,
            span: p.name_span.into(),
            pattern_source_span: None,
            append_op: None,
            element_index: None,
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
    let mut cursor_y = y;
    for (i, stmt) in stmts.iter().enumerate() {
        project_stmt(g, index, src, stmt, base + i + 1, x, cursor_y);
        cursor_y += stmt_row_step(stmt);
    }
}

/// Vertical spacing before the next sibling statement. The default slot
/// (130px) has room for a couple of data-provider rows below a binding; a
/// list/fan-out initializer with more items renders taller than that, so
/// widen the gap or its node body collides with the next statement's own
/// data-provider node (multi-input node body height grows with item count,
/// e.g. after `append_multi_input`).
fn stmt_row_step(stmt: &Stmt) -> i32 {
    let items = match stmt {
        Stmt::Val(b) => multi_input_item_count(&b.init),
        Stmt::Assign { value, .. } => multi_input_item_count(value),
        _ => 0,
    };
    130 + (items.saturating_sub(2) as i32) * 55
}

fn multi_input_item_count(expr: &Expr) -> usize {
    match expr {
        Expr::ListLit(items, _) => items.len(),
        Expr::FanOut { items, .. } => items.len(),
        _ => 0,
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
                "function_exec",
                &b.name,
                b.name_span.into(),
                x,
                y,
                vec!["local"],
                vec!["rename_binding", "edit_inline_expr", "source_jump"],
            );
            if let Some(node) = g.nodes.iter_mut().find(|node| node.id == node_id) {
                node.meta_json = meta_attr_json(b.meta.as_ref());
            }
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
                "function_exec",
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
            let _ = project_expr_node(g, index, src, e, ordinal, x, y, true);
        }
        Stmt::Return(expr, span) => {
            let node_id = format!("{}:stmt:{ordinal}:return", g.graph_id);
            add_node(
                g,
                &node_id,
                "return",
                "control",
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
            let mut affordances = vec!["edit_inline_expr", "source_jump"];
            let is_pattern_test = matches!(ifs.cond, Expr::PatternTest { .. });
            if is_pattern_test {
                affordances.push("add_pattern_arm");
            }
            let title = if is_pattern_test { "if ==" } else { "if" };
            add_node(
                g,
                &node_id,
                "branch",
                "control",
                title,
                ifs.cond.span().into(),
                x,
                y,
                vec!["control"],
                affordances,
            );
            let cond = add_pin(g, &node_id, "cond", "input", "Bool", "", false);
            if matches!(ifs.cond, Expr::PatternTest { .. }) {
                let span = pattern_arm_edit_span(&ifs.cond);
                add_arm_pin(
                    g,
                    &node_id,
                    "arm1",
                    &pattern_pin_label(src, &ifs.cond),
                    span,
                );
            }
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
                "control",
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
                "control",
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
                "control",
                "counted loop",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["add_pattern_arm", "edit_inline_expr", "source_jump"],
            );
            let initializer_span = Span::new(init.name_span.start, init.init.span().end);
            let init_pin = add_pin(g, &node_id, "initializer", "input", "Value", "", false);
            connect_expr_to_input_with_span(g, index, src, &init.init, initializer_span, ordinal * 10, "initializer", &node_id, &init_pin, x - 220, y);
            let cond_pin = add_pin(g, &node_id, "condition", "input", "Bool", "", false);
            connect_expr_to_input(g, index, src, cond, ordinal * 10 + 1, "condition", &node_id, &cond_pin, x - 220, y + 30);
            if let Some(step) = step {
                let afterthought = match step.as_ref() {
                    Stmt::Assign { target, value, .. } => {
                        Some((value, Span::new(target.span().start, expr_source_end(value))))
                    }
                    Stmt::Expr(value) => Some((value, value.span())),
                    _ => None,
                };
                if let Some((afterthought, afterthought_span)) = afterthought {
                    let pin = add_pin(g, &node_id, "afterthought", "input", "Value", "", false);
                    connect_expr_to_input_with_span(g, index, src, afterthought, afterthought_span, ordinal * 10 + 2, "afterthought", &node_id, &pin, x - 220, y + 60);
                }
            }
            project_stmt_block(g, index, src, body, ordinal * 100 + 30, x + 230, y + 200);
        }
        Stmt::For {
            var,
            var2,
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
                "control",
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
            if let Some((var2, _)) = var2 {
                let output = add_pin(g, &node_id, var2, "output", iter_ty, "", false);
                g.local_pins.insert(var2.clone(), output);
                g.local_types.insert(var2.clone(), iter_ty.to_string());
            }
            match kind {
                AST::ForKind::Range { start, end, step } => {
                    let start_pin = add_pin(g, &node_id, "range_start", "input", "Int", "", false);
                    connect_expr_to_input(g, index, src, start, ordinal * 10, "range_start", &node_id, &start_pin, x - 220, y);
                    let end_pin = add_pin(g, &node_id, "range_end", "input", "Int", "", false);
                    connect_expr_to_input(g, index, src, end, ordinal * 10 + 1, "range_end", &node_id, &end_pin, x - 220, y + 30);
                    if let Some(step) = step {
                        let stride_pin = add_pin(g, &node_id, "stride", "input", "Int", "", false);
                        connect_expr_to_input(g, index, src, step, ordinal * 10 + 2, "stride", &node_id, &stride_pin, x - 220, y + 60);
                    }
                }
                AST::ForKind::In { collection, step } => {
                    let source_pin = add_pin(g, &node_id, "source", "input", "Iterable", "", false);
                    connect_expr_to_input(g, index, src, collection, ordinal * 10, "source", &node_id, &source_pin, x - 220, y);
                    if let Some(step) = step {
                        let stride_pin = add_pin(g, &node_id, "stride", "input", "Int", "", false);
                        connect_expr_to_input(g, index, src, step, ordinal * 10 + 1, "stride", &node_id, &stride_pin, x - 220, y + 30);
                    }
                }
            }
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
            let subjectless = AST::is_subjectless_guard(subject, *span);
            let node_id = format!("{}:stmt:{ordinal}:dispatch", g.graph_id);
            add_node(
                g,
                &node_id,
                "function",
                "control",
                if subjectless { "if guards" } else { "if ==" },
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["add_pattern_arm", "edit_inline_expr", "source_jump"],
            );
            if !subjectless {
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
            }
            for (i, arm) in arms.iter().enumerate() {
                add_arm_pin(
                    g,
                    &node_id,
                    &format!("arm{}", i + 1),
                    &if subjectless {
                        balance_closing_parens(snippet(src, arm.cond.span()).trim())
                    } else {
                        dispatch_arm_pattern_label(src, &arm.cond)
                    },
                    arm.cond.span().into(),
                );
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
                "control",
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
                json_str(audit.as_deref().unwrap_or("@Unsafe")),
                span_json((*span).into())
            ));
            project_stmt_block(g, index, src, body, ordinal * 100 + 95, x + 230, y + 70);
        }
        Stmt::Impure { reason, body, span } => {
            add_region(
                g,
                ordinal,
                "impure",
                reason.as_deref().unwrap_or("@Impure"),
                *span,
            );
            project_stmt_block(g, index, src, body, ordinal * 100 + 100, x + 230, y + 70);
        }
        Stmt::Reactive { body, span } => {
            add_region(g, ordinal, "reactive", "@Reactive", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 110, x + 230, y + 70);
        }
        Stmt::Shield { body, span } => {
            add_region(g, ordinal, "shield", "@Shield", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 115, x + 230, y + 70);
        }
        Stmt::Off { body, .. } | Stmt::DebugOnly { body, .. } => {
            project_stmt_block(g, index, src, body, ordinal * 100 + 125, x + 230, y + 70);
        }
        Stmt::Region {
            name, body, span, ..
        } => {
            add_region(g, ordinal, "region", name, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 130, x + 230, y + 70);
        }
        Stmt::Policy { declarations, body, span } => {
            let label = declarations.iter().map(|d| d.key.name()).collect::<Vec<_>>().join(", ");
            add_region(g, ordinal, "policy", &format!("@Policy({label})"), *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 135, x + 230, y + 70);
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
                "control",
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
        | Stmt::AssumeDet { body, span, .. }
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
                "function_exec",
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
                "function_exec",
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

fn pattern_pin_label(src: &str, expr: &Expr) -> String {
    let raw = snippet(src, expr.span());
    let balanced = balance_closing_parens(raw.trim());
    if matches!(expr, Expr::PatternTest { .. }) {
        if let Some(pos) = balanced.find("==") {
            return balanced[pos..].trim().to_string();
        }
        return format!("== {}", balanced.trim());
    }
    balanced
}

/// Label for a dispatch-form (`if subject == { ... }`) arm pattern. Unlike
/// `pattern_pin_label`, dispatch arms never carry their own `==` (the
/// operator belongs to the dispatch header, not the arm line) and the
/// leading dot on enum-variant patterns (D-ENUMDOT1) is a source spelling
/// detail, not part of the arm's identity — so both are stripped for the
/// Canvas label.
fn dispatch_arm_pattern_label(src: &str, expr: &Expr) -> String {
    let raw = snippet(src, expr.span());
    let mut balanced = balance_closing_parens(raw.trim());
    if let Some(pos) = balanced.find("==") {
        balanced = balanced[pos + 2..].trim().to_string();
    }
    balanced.strip_prefix('.').unwrap_or(&balanced).to_string()
}

fn pattern_arm_edit_span(expr: &Expr) -> SourceSpan {
    match expr {
        Expr::PatternTest { pattern, .. } => pattern.span().into(),
        _ => expr.span().into(),
    }
}

fn balance_closing_parens(s: &str) -> String {
    let mut out = s.to_string();
    let opens = s.chars().filter(|c| *c == '(').count();
    let closes = s.chars().filter(|c| *c == ')').count();
    for _ in closes..opens {
        out.push(')');
    }
    out
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
    connect_expr_to_input_with_span(
        g,
        index,
        src,
        expr,
        expr.span(),
        ordinal,
        role,
        owner_node_id,
        input_pin,
        x,
        y,
    );
}

fn expr_source_end(expr: &Expr) -> usize {
    match expr {
        // Sema expands compound assignment with the target span on the new
        // binary node; the original RHS child retains the source end.
        Expr::Binary(_, left, right, _) => expr_source_end(left).max(expr_source_end(right)),
        _ => expr.span().end,
    }
}

#[allow(clippy::too_many_arguments)]
fn connect_expr_to_input_with_span(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    inline_span: Span,
    ordinal: usize,
    role: &str,
    owner_node_id: &str,
    input_pin: &str,
    x: i32,
    y: i32,
) {
    let provider_y = data_provider_y(y, ordinal);
    if let Some(out) = project_value_node(g, index, src, expr, ordinal, role, x, provider_y) {
        add_inline(g, owner_node_id, ordinal, role, src, inline_span);
        add_wire_with_span(g, &out, input_pin, "data", Some(expr.span().into()));
    } else if pure_leaf(expr) {
        add_inline(g, owner_node_id, ordinal, role, src, inline_span);
        wire_ident_refs(g, expr, input_pin);
    } else if let Some(out) = project_expr_node(g, index, src, expr, ordinal, x, provider_y, false) {
        add_wire_with_span(g, &out, input_pin, "data", Some(expr.span().into()));
    }
}

fn data_provider_y(y: i32, ordinal: usize) -> i32 {
    y + 96 + ((ordinal % 2) as i32 * 18)
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
    if let Expr::Ident(name, span) = expr {
        if let Some(pin) = g.getter_pins.get(name).cloned() {
            return Some(pin);
        }
        let ty = expr_type(g, index, expr);
        let node_id = format!("{}:value:get:{}", g.graph_id, canvas_ident_fragment(name));
        add_node(
            g,
            &node_id,
            "variable_get",
            "value",
            name,
            (*span).into(),
            x,
            y,
            vec!["read"],
            vec!["edit_inline_expr", "source_jump"],
        );
        let pin = add_pin(g, &node_id, name, "output", &ty, "", false);
        g.getter_pins.insert(name.clone(), pin.clone());
        return Some(pin);
    }
    let (kind, title, badges) = match expr {
        Expr::Int(_, _, _, _)
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
        "value",
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

fn canvas_ident_fragment(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn project_expr_node(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    ordinal: usize,
    x: i32,
    y: i32,
    exec_context: bool,
) -> Option<String> {
    match expr {
        Expr::Call(c) => {
            let node_id = format!("{}:expr:{ordinal}:call:{}", g.graph_id, c.name);
            let archetype = if exec_context || call_has_effects(index, &c.name) {
                "function_exec"
            } else {
                "function_pure"
            };
            add_node(
                g,
                &node_id,
                "function",
                archetype,
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
            let kind = if variant_like { "variant" } else { "function" };
            let archetype = if variant_like {
                "function_pure"
            } else if exec_context || call_has_effects(index, method) {
                "function_exec"
            } else {
                "function_pure"
            };
            let title = if variant_like {
                method.clone()
            } else {
                format!(".{method}")
            };
            add_node(
                g,
                &node_id,
                kind,
                archetype,
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
                "control",
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
            if let Some(out) = project_expr_node(g, index, src, inner, ordinal, x - 180, y, false) {
                add_wire(g, &out, &input, "fallible");
            }
            Some(add_pin(g, &node_id, "ok", "output", "unknown", "", false))
        }
        Expr::OrFallback { value, .. } => {
            project_expr_node(g, index, src, value, ordinal, x, y, exec_context)
        }
        Expr::ListLit(items, span) => {
            let node_id = format!("{}:expr:{ordinal}:list", g.graph_id);
            add_node(
                g,
                &node_id,
                "expr",
                "function_pure",
                "list",
                (*span).into(),
                x,
                y,
                vec!["multi-input"],
                vec!["append_multi_input", "edit_inline_expr", "source_jump"],
            );
            for (i, item) in items.iter().enumerate() {
                let ty = expr_type(g, index, item);
                let input = add_pin(
                    g,
                    &node_id,
                    &format!("item{}", i + 1),
                    "input",
                    &ty,
                    "",
                    false,
                );
                set_pin_source_span(g, &input, item.span().into());
                set_pin_append(g, &input, "remove_multi_input_element", i);
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    item,
                    ordinal * 1000 + i + 1,
                    &format!("item{}", i + 1),
                    &node_id,
                    &input,
                    x - 220,
                    y + i as i32 * 74,
                );
            }
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
        Expr::FanOut {
            callee,
            items,
            span,
        } => {
            let node_id = format!("{}:expr:{ordinal}:fanout", g.graph_id);
            add_node(
                g,
                &node_id,
                "function",
                "function_pure",
                "fanout",
                (*span).into(),
                x,
                y,
                vec!["multi-input"],
                vec!["append_multi_input", "edit_inline_expr", "source_jump"],
            );
            let callee_pin = add_pin(g, &node_id, "callee", "input", "Fn", "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                callee,
                ordinal * 1000,
                "callee",
                &node_id,
                &callee_pin,
                x - 220,
                y,
            );
            for (i, item) in items.iter().enumerate() {
                let ty = expr_type(g, index, item);
                let input = add_pin(
                    g,
                    &node_id,
                    &format!("item{}", i + 1),
                    "input",
                    &ty,
                    "",
                    false,
                );
                set_pin_source_span(g, &input, item.span().into());
                set_pin_append(g, &input, "remove_multi_input_element", i);
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    item,
                    ordinal * 1000 + i + 1,
                    &format!("item{}", i + 1),
                    &node_id,
                    &input,
                    x - 220,
                    y + (i as i32 + 1) * 74,
                );
            }
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
        _ => {
            let node_id = format!("{}:expr:{ordinal}:expr", g.graph_id);
            let title = expr_title(expr);
            add_node(
                g,
                &node_id,
                "expr",
                "function_pure",
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
