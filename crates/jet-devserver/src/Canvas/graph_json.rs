use jet_driver::Diagnostics::Span;
use jet_driver::AST;
use jet_foundation::Names::{NameLedger, NameVisibility};
use jet_semindex::SourceSpan;

use super::debug_source_git::span_overlaps;
use super::graph_helpers::{line_start, snippet};
use super::schema_api::{GraphBuilder, InlineRec, NodeRec, PinRec, WireRec};
use super::validation_json::{attr_bounds, attr_span, attr_string, json_str, json_strs, span_json};

#[path = "node_catalog.rs"]
pub(super) mod node_catalog;

pub(super) fn add_node(
    g: &mut GraphBuilder,
    id: &str,
    descriptor_id: &str,
    title: &str,
    span: SourceSpan,
    x: i32,
    y: i32,
    badges: Vec<&str>,
    affordances: Vec<&str>,
) {
    let descriptor = node_catalog::descriptor_for_id(descriptor_id);
    g.nodes.push(NodeRec {
        id: id.to_string(),
        kind: descriptor.kind.to_string(),
        archetype: descriptor.archetype.to_string(),
        title: title.to_string(),
        span,
        x,
        y,
        badges: badges.into_iter().map(str::to_string).collect(),
        affordances: affordances.into_iter().map(str::to_string).collect(),
        meta_json: None,
    });
}

pub(super) fn add_pin(
    g: &mut GraphBuilder,
    node_id: &str,
    name: &str,
    direction: &str,
    ty: &str,
    ability: &str,
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
        ability: ability.to_string(),
        fallible,
        effect_grant_need: None,
        span,
        pattern_source_span: None,
        append_op: None,
        element_index: None,
    });
    id
}

pub(super) fn add_arm_pin(
    g: &mut GraphBuilder,
    node_id: &str,
    name: &str,
    pattern_source: &str,
    pattern_span: SourceSpan,
) -> String {
    let id = format!("{node_id}:output:{name}");
    if !g.pins.iter().any(|pin| pin.id == id) {
        g.pins.push(PinRec {
            id: id.clone(),
            node_id: node_id.to_string(),
            name: name.to_string(),
            direction: "output".to_string(),
            ty: "exec".to_string(),
            role: Some("arm".to_string()),
            pattern_source: Some(pattern_source.to_string()),
            ability: "control".to_string(),
            fallible: false,
            effect_grant_need: None,
            span: pattern_span,
            pattern_source_span: Some(pattern_span),
            append_op: Some("edit_pattern_arm".to_string()),
            element_index: None,
        });
    }
    id
}

pub(super) fn set_pin_source_span(g: &mut GraphBuilder, pin_id: &str, span: SourceSpan) {
    if let Some(pin) = g.pins.iter_mut().find(|pin| pin.id == pin_id) {
        pin.span = span;
    }
}

pub(super) fn set_pin_append(g: &mut GraphBuilder, pin_id: &str, op: &str, index: usize) {
    if let Some(pin) = g.pins.iter_mut().find(|pin| pin.id == pin_id) {
        pin.append_op = Some(op.to_string());
        pin.element_index = Some(index);
    }
}

pub(super) fn add_wire(g: &mut GraphBuilder, from_pin: &str, to_pin: &str, kind: &str) {
    add_wire_with_span(g, from_pin, to_pin, kind, None);
}

pub(super) fn add_wire_with_span(
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

pub(super) fn add_region(
    g: &mut GraphBuilder,
    ordinal: usize,
    kind: &str,
    title: &str,
    span: Span,
) {
    g.regions.push(format!(
        "{{\"region_id\":{},\"kind\":{},\"title\":{},\"source_span\":{}}}",
        json_str(&format!("{}:region:{ordinal}:{kind}", g.graph_id)),
        json_str(kind),
        json_str(title),
        span_json(span.into())
    ));
}

pub(super) fn add_authority_region(
    g: &mut GraphBuilder,
    ordinal: usize,
    title: &str,
    granted_effects: &[String],
    binding: Option<&str>,
    span: Span,
) {
    let authority = binding
        .map(|name| format!("Authority handle `{name}`"))
        .unwrap_or_else(|| "lexical #FX authority".to_string());
    g.regions.push(format!(
        "{{\"region_id\":{},\"kind\":\"authority\",\"title\":{},\"required_effects\":[],\"granted_effects\":{},\"denied_effects\":[],\"authority\":{},\"source_span\":{}}}",
        json_str(&format!("{}:region:{ordinal}:authority", g.graph_id)),
        json_str(title),
        json_strs(granted_effects),
        json_str(&authority),
        span_json(span.into())
    ));
}

pub(super) fn meta_attr_json(meta: Option<&AST::MetaAttr>) -> Option<String> {
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

pub(super) fn add_source_comment_regions(g: &mut GraphBuilder, src: &str, f: &AST::Func) {
    let func_span = func_source_span(f);
    for hint in canvas_comment_hints(src) {
        if !hint_belongs_to_function(src, hint.anchor, hint.hint_span, func_span) {
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
        if !hint_belongs_to_function(src, hint.anchor, hint.hint_span, func_span) {
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

fn hint_belongs_to_function(
    src: &str,
    anchor: SourceSpan,
    hint_span: SourceSpan,
    func_span: SourceSpan,
) -> bool {
    if span_overlaps(anchor, func_span) || span_overlaps(hint_span, func_span) {
        return true;
    }
    if hint_span.start < func_span.end {
        return false;
    }
    hint_span.start < next_function_start(src, func_span.end)
}

fn next_function_start(src: &str, offset: usize) -> usize {
    let mut cursor = offset.min(src.len());
    for line in src[cursor..].split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub(package) fn ")
        {
            return cursor + line.len() - trimmed.len();
        }
        cursor += line.len();
    }
    src.len()
}

pub(super) fn add_execution_overlay(g: &mut GraphBuilder, src: &str, body: &[AST::Stmt]) {
    let exec_nodes = g
        .nodes
        .iter()
        .filter(|node| node_wants_exec(node))
        .cloned()
        .collect::<Vec<_>>();

    for node in &exec_nodes {
        if node.kind != "entry" {
            ensure_exec_pin(g, &node.id, "exec", "input", node.span, None);
        }
        if node_has_arm_pins(g, &node.id) {
            ensure_exec_pin(g, &node.id, "else", "output", node.span, Some("else"));
        } else if node.kind == "loop" {
            ensure_exec_pin(g, &node.id, "body", "output", node.span, Some("loop_body"));
            ensure_exec_pin(g, &node.id, "done", "output", node.span, Some("loop_done"));
        } else if node.kind == "return" {
            ensure_exec_pin(
                g,
                &node.id,
                "return",
                "output",
                node.span,
                Some("early_return"),
            );
        } else if node.kind == "branch" || (node.kind == "function" && node.archetype == "control")
        {
            ensure_exec_pin(g, &node.id, "then", "output", node.span, None);
            ensure_exec_pin(g, &node.id, "else", "output", node.span, Some("else"));
        } else if node.kind != "return" {
            ensure_exec_pin(g, &node.id, "then", "output", node.span, None);
        }
    }

    let Some(entry) = g.nodes.iter().find(|node| node.kind == "entry") else {
        return;
    };
    let entry_id = entry.id.clone();
    let entry_span = entry.span;
    let flow = execution_block(g, src, body);
    if let Some(input) = flow.entry {
        add_control_wire_once(
            g,
            &format!("{entry_id}:output:then"),
            &input.pin,
            entry_span,
            input.span,
        );
    }
}

fn node_has_arm_pins(g: &GraphBuilder, node_id: &str) -> bool {
    g.pins.iter().any(|pin| {
        pin.node_id == node_id && pin.direction == "output" && pin.role.as_deref() == Some("arm")
    })
}

#[derive(Clone)]
struct ExecutionEndpoint {
    pin: String,
    span: SourceSpan,
}

#[derive(Default)]
struct ExecutionFlow {
    entry: Option<ExecutionEndpoint>,
    exits: Vec<ExecutionEndpoint>,
}

fn execution_block(g: &mut GraphBuilder, src: &str, stmts: &[AST::Stmt]) -> ExecutionFlow {
    let mut flow = ExecutionFlow::default();
    let mut exits = Vec::new();
    let mut reachable = true;

    for stmt in stmts {
        if !reachable {
            break;
        }
        let current = execution_stmt(g, src, stmt);
        let Some(entry) = current.entry.clone() else {
            if !current.exits.is_empty() {
                exits = current.exits;
                reachable = true;
            }
            continue;
        };
        if flow.entry.is_none() {
            flow.entry = Some(entry.clone());
        }
        for previous in &exits {
            add_control_wire_once(g, &previous.pin, &entry.pin, previous.span, entry.span);
        }
        exits = current.exits;
        reachable = !exits.is_empty();
    }

    flow.exits = exits;
    flow
}

fn execution_stmt(g: &mut GraphBuilder, src: &str, stmt: &AST::Stmt) -> ExecutionFlow {
    match stmt {
        AST::Stmt::Switch {
            arms, else_body, ..
        }
        | AST::Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            let Some(node) = direct_execution_node(g, stmt) else {
                return ExecutionFlow::default();
            };
            let mut exits = Vec::new();
            for (index, arm) in arms.iter().enumerate() {
                let output_name = if node_has_arm_pins(g, &node.id) {
                    format!("arm{}", index + 1)
                } else if index == 0 {
                    "then".to_string()
                } else {
                    String::new()
                };
                let output = (!output_name.is_empty())
                    .then(|| exec_output(g, &node.id, &output_name))
                    .flatten();
                append_branch_path(g, src, output, &arm.body, node.span, &mut exits);
            }
            let else_output = exec_output(g, &node.id, "else");
            if let Some(body) = else_body.as_deref() {
                append_branch_path(g, src, else_output, body, node.span, &mut exits);
            } else if let Some(output) = else_output {
                exits.push(ExecutionEndpoint {
                    pin: output,
                    span: node.span,
                });
            }
            ExecutionFlow {
                entry: exec_input(&node),
                exits,
            }
        }
        AST::Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            let Some(node) = direct_execution_node(g, stmt) else {
                return ExecutionFlow::default();
            };
            let mut exits = Vec::new();
            let then_output = exec_output(g, &node.id, "then");
            append_branch_path(g, src, then_output, then_body, node.span, &mut exits);
            if let Some(body) = else_body.as_deref() {
                let else_output = exec_output(g, &node.id, "else");
                append_branch_path(g, src, else_output, body, node.span, &mut exits);
            } else if let Some(output) = exec_output(g, &node.id, "else") {
                exits.push(ExecutionEndpoint {
                    pin: output,
                    span: node.span,
                });
            }
            ExecutionFlow {
                entry: exec_input(&node),
                exits,
            }
        }
        AST::Stmt::While { body, .. }
        | AST::Stmt::For { body, .. }
        | AST::Stmt::Loop { body, .. }
        | AST::Stmt::CountedLoop { body, .. } => {
            let Some(node) = direct_execution_node(g, stmt) else {
                return ExecutionFlow::default();
            };
            let body_output = exec_output(g, &node.id, "body");
            let body_flow = execution_block(g, src, body);
            if let Some(body_output) = body_output {
                if let Some(body_entry) = body_flow.entry {
                    add_control_wire_once(
                        g,
                        &body_output,
                        &body_entry.pin,
                        node.span,
                        body_entry.span,
                    );
                }
                for exit in body_flow.exits {
                    add_control_wire_once(
                        g,
                        &exit.pin,
                        &format!("{}:input:exec", node.id),
                        exit.span,
                        node.span,
                    );
                }
            }
            ExecutionFlow {
                entry: exec_input(&node),
                exits: exec_output(g, &node.id, "done")
                    .into_iter()
                    .map(|pin| ExecutionEndpoint {
                        pin,
                        span: node.span,
                    })
                    .collect(),
            }
        }
        AST::Stmt::Switched { marker, .. } if AST::switched_off(marker) => ExecutionFlow::default(),
        AST::Stmt::Switched { body, .. } => execution_block(g, src, body),
        AST::Stmt::Unsafe { body, .. }
        | AST::Stmt::Impure { body, .. }
        | AST::Stmt::Reactive { body, .. }
        | AST::Stmt::Shield { body, .. }
        | AST::Stmt::Region { body, .. }
        | AST::Stmt::Policy { body, .. }
        | AST::Stmt::TaskGroup { body, .. }
        | AST::Stmt::Layout { body, .. }
        | AST::Stmt::AuthorityScope { body, .. }
        | AST::Stmt::ComptimeBlock { body, .. }
        | AST::Stmt::ContextBlock { body, .. }
        | AST::Stmt::Live { body, .. }
        | AST::Stmt::AssumeDet { body, .. }
        | AST::Stmt::Transact { body, .. }
        | AST::Stmt::ScopeMember { body, .. } => execution_block(g, src, body),
        _ => {
            let Some(node) = direct_execution_node(g, stmt) else {
                return ExecutionFlow::default();
            };
            let exits = if matches!(
                stmt,
                AST::Stmt::Return(..)
                    | AST::Stmt::Break(..)
                    | AST::Stmt::BreakValue(..)
                    | AST::Stmt::BreakLabel(..)
                    | AST::Stmt::BreakLabelValue(..)
                    | AST::Stmt::Continue(..)
                    | AST::Stmt::ContinueLabel(..)
            ) {
                Vec::new()
            } else {
                exec_output(g, &node.id, "then")
                    .into_iter()
                    .map(|pin| ExecutionEndpoint {
                        pin,
                        span: node.span,
                    })
                    .collect()
            };
            ExecutionFlow {
                entry: exec_input(&node),
                exits,
            }
        }
    }
}

fn append_branch_path(
    g: &mut GraphBuilder,
    src: &str,
    output: Option<String>,
    body: &[AST::Stmt],
    owner_span: SourceSpan,
    exits: &mut Vec<ExecutionEndpoint>,
) {
    let Some(output) = output else {
        return;
    };
    let body_flow = execution_block(g, src, body);
    if let Some(entry) = body_flow.entry {
        add_control_wire_once(g, &output, &entry.pin, owner_span, entry.span);
        exits.extend(body_flow.exits);
    } else {
        exits.push(ExecutionEndpoint {
            pin: output,
            span: owner_span,
        });
    }
}

fn direct_execution_node(g: &GraphBuilder, stmt: &AST::Stmt) -> Option<NodeRec> {
    let anchor = stmt_execution_anchor(stmt)?;
    g.nodes
        .iter()
        .filter(|node| node_wants_exec(node) && node.kind != "entry")
        .find(|node| match stmt {
            AST::Stmt::Val(binding) => {
                node.kind == "binding"
                    && node.title == binding.name
                    && node.span.start == binding.name_span.start
            }
            _ => node.span.start == anchor.start && node.span.end == anchor.end,
        })
        .cloned()
}

fn stmt_execution_anchor(stmt: &AST::Stmt) -> Option<SourceSpan> {
    Some(match stmt {
        AST::Stmt::Val(binding) => binding.name_span.into(),
        AST::Stmt::Assign { target, .. } => target.span().into(),
        AST::Stmt::Expr(expr) => expr_execution_anchor(expr),
        AST::Stmt::Return(_, span)
        | AST::Stmt::Break(span)
        | AST::Stmt::Continue(span)
        | AST::Stmt::BreakLabel(_, span)
        | AST::Stmt::ContinueLabel(_, span)
        | AST::Stmt::BreakValue(_, span)
        | AST::Stmt::BreakLabelValue(_, _, _, span)
        | AST::Stmt::DeferClose { span, .. }
        | AST::Stmt::Yield(_, span)
        | AST::Stmt::Unsafe { span, .. }
        | AST::Stmt::Impure { span, .. }
        | AST::Stmt::Reactive { span, .. }
        | AST::Stmt::Shield { span, .. }
        | AST::Stmt::Region { span, .. }
        | AST::Stmt::Policy { span, .. }
        | AST::Stmt::TaskGroup { span, .. }
        | AST::Stmt::Layout { span, .. }
        | AST::Stmt::AuthorityScope { span, .. }
        | AST::Stmt::ComptimeBlock { span, .. }
        | AST::Stmt::ContextBlock { span, .. }
        | AST::Stmt::Live { span, .. }
        | AST::Stmt::AssumeDet { span, .. }
        | AST::Stmt::Transact { span, .. }
        | AST::Stmt::ScopeMember { span, .. }
        | AST::Stmt::Switch { span, .. }
        | AST::Stmt::ComptimeSwitch { span, .. }
        | AST::Stmt::ComptimeIf { span, .. }
        | AST::Stmt::While { span, .. }
        | AST::Stmt::For { span, .. }
        | AST::Stmt::Loop { span, .. }
        | AST::Stmt::CountedLoop { span, .. }
        | AST::Stmt::Switched { span, .. } => (*span).into(),
    })
}

fn expr_execution_anchor(expr: &AST::Expr) -> SourceSpan {
    match expr {
        AST::Expr::Call(call) => call.name_span.into(),
        AST::Expr::MethodCall { method_span, .. } => (*method_span).into(),
        AST::Expr::Try(inner, span, ..) => {
            let _ = inner;
            (*span).into()
        }
        AST::Expr::OrFallback { value, .. } => expr_execution_anchor(value),
        _ => expr.span().into(),
    }
}

fn exec_input(node: &NodeRec) -> Option<ExecutionEndpoint> {
    (node.kind != "entry").then(|| ExecutionEndpoint {
        pin: format!("{}:input:exec", node.id),
        span: node.span,
    })
}

fn exec_output(g: &GraphBuilder, node_id: &str, name: &str) -> Option<String> {
    g.pins
        .iter()
        .find(|pin| pin.node_id == node_id && pin.direction == "output" && pin.name == name)
        .map(|pin| pin.id.clone())
}

fn add_control_wire_once(
    g: &mut GraphBuilder,
    from_pin: &str,
    to_pin: &str,
    from_span: SourceSpan,
    to_span: SourceSpan,
) {
    if g.wires
        .iter()
        .any(|wire| wire.kind == "control" && wire.from_pin == from_pin && wire.to_pin == to_pin)
    {
        return;
    }
    add_control_wire(g, from_pin, to_pin, from_span, to_span);
}

fn node_wants_exec(node: &NodeRec) -> bool {
    matches!(
        node.archetype.as_str(),
        "entry" | "function_exec" | "control"
    )
}

fn ensure_exec_pin(
    g: &mut GraphBuilder,
    node_id: &str,
    name: &str,
    direction: &str,
    span: SourceSpan,
    role: Option<&str>,
) -> String {
    let id = format!("{node_id}:{direction}:{name}");
    if !g.pins.iter().any(|pin| pin.id == id) {
        g.pins.push(PinRec {
            id: id.clone(),
            node_id: node_id.to_string(),
            name: name.to_string(),
            direction: direction.to_string(),
            ty: "exec".to_string(),
            role: role.map(str::to_string),
            pattern_source: None,
            ability: "control".to_string(),
            fallible: false,
            effect_grant_need: None,
            span,
            pattern_source_span: None,
            append_op: None,
            element_index: None,
        });
    }
    id
}

pub(super) fn func_source_span(f: &AST::Func) -> SourceSpan {
    f.span.into()
}

#[derive(Clone)]
pub(super) struct CommentHint {
    pub(super) anchor: SourceSpan,
    pub(super) hint_span: SourceSpan,
    pub(super) title: String,
    pub(super) color: String,
    pub(super) alpha: String,
    pub(super) bounds: (i32, i32, i32, i32),
}

pub(super) fn canvas_comment_hints(src: &str) -> Vec<CommentHint> {
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

pub(super) fn canvas_collapse_hints(src: &str) -> Vec<CommentHint> {
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

pub(super) fn add_inline(
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

pub(super) fn graph_to_json(
    g: &GraphBuilder,
    f: &AST::Func,
    src: &str,
    visibility: &'static str,
) -> String {
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
        function_metadata_json(src, f, visibility),
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

fn function_metadata_json(src: &str, f: &AST::Func, visibility: &'static str) -> String {
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
                "{{\"name\":{},\"type\":{},\"ability\":{},\"default\":{},\"default_source\":{},\"source_span\":{}}}",
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
    let ret = f.effective_return_type().name();
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
    let effect_via = f
        .effect_via
        .as_ref()
        .map(|(param, _)| json_str(param))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"name\":{},\"signature\":{},\"visibility\":{},\"docs\":{},\"pure\":{},\"unsafe\":{},\"effects\":[{}],\"effect_via\":{},\"returns\":{},\"params\":[{}],\"meta\":{},\"source_span\":{},\"edit_affordances\":[\"rename_function\",\"edit_function_signature\",\"create_function\",\"source_jump\"]}}",
        json_str(&f.name),
        json_str(&function_signature_text(src, f, visibility)),
        json_str(visibility),
        json_str(&doc_comment_before(src, f.name_span.start)),
        if f.is_pure { "true" } else { "false" },
        if f.is_unsafe { "true" } else { "false" },
        effects,
        effect_via,
        json_str(&ret),
        params,
        meta_attr_json(f.meta.as_ref()).unwrap_or_else(|| "null".to_string()),
        span_json(func_source_span(f))
    )
}

pub(super) fn ledger_function_visibility(
    ledger: &NameLedger,
    module_idx: usize,
    name: &str,
) -> &'static str {
    match ledger
        .declaration(module_idx, name)
        .map(|declaration| declaration.visibility)
    {
        Some(NameVisibility::Package) => "package",
        Some(NameVisibility::Public) => "public",
        Some(NameVisibility::Private) => "private",
        None => "private",
    }
}

fn function_signature_text(src: &str, f: &AST::Func, visibility: &'static str) -> String {
    let mut out = String::new();
    match visibility {
        "package" => out.push_str("pub(package) "),
        "public" => out.push_str("pub "),
        _ => {}
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
                    s.push('{');
                    s.push_str(&snippet(src, default.span()));
                    s.push('}');
                }
                s
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push(')');
    out.push(' ');
    out.push_str(&f.effective_return_type().name());
    if let Some((param, _)) = &f.effect_via {
        out.push_str(" -[via ");
        out.push_str(param);
        out.push_str("]>");
    } else if let Some(effects) = &f.declared_effects {
        out.push_str(" -[");
        out.push_str(
            &effects
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("]>");
    } else if f.is_pure {
        out.push_str(" -[]>");
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
        .any(|r| r.contains("\"kind\":\"task.group\""))
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
            .any(|r| {
                r.contains("\"kind\":\"authority\"")
                    || r.contains("\"kind\":\"grant\"")
            })
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
    let descriptor = node_catalog::descriptor_for(&n.kind, &n.archetype);
    let meta = n
        .meta_json
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"node_id\":{},\"node_descriptor_id\":{},\"kind\":{},\"archetype\":{},\"title\":{},\"source_span\":{},\"layout\":{{\"x\":{},\"y\":{}}},\"badges\":[{}],\"edit_affordances\":[{}],\"meta\":{}}}",
        json_str(&n.id),
        json_str(descriptor.id),
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
    let pattern_source_span = p
        .pattern_source_span
        .map(span_json)
        .unwrap_or_else(|| "null".to_string());
    let append_op = p
        .append_op
        .as_ref()
        .map(|s| json_str(s))
        .unwrap_or_else(|| "null".to_string());
    let element_index = p
        .element_index
        .map(|n| n.to_string())
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"pin_id\":{},\"node_id\":{},\"name\":{},\"direction\":{},\"type\":{},\"role\":{},\"pattern_source\":{},\"pattern_source_span\":{},\"append_op\":{},\"element_index\":{},\"ability\":{},\"fallible\":{},\"effect_grant_need\":{},\"source_span\":{}}}",
        json_str(&p.id),
        json_str(&p.node_id),
        json_str(&p.name),
        json_str(&p.direction),
        json_str(&p.ty),
        role,
        pattern_source,
        pattern_source_span,
        append_op,
        element_index,
        json_str(&p.ability),
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
