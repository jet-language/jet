use std::fs;
use std::path::Path;

use jet_driver::Diagnostics::{Diagnostic, Severity, Span, TextEdit};
use jet_driver::AST::{self, Expr};
use jet_semindex::{SemIndex, SourceSpan, SymbolKind};

use super::graph_json::add_wire_with_span;
use super::project_scan::project_file;
use super::query_actions::canvas_authority_context;
use super::schema_api::{
    ACTION_SCHEMA_VERSION, EDIT_SCHEMA_VERSION, GraphBuilder, NodeQueryRef,
    PROJECT_SCHEMA_VERSION, Projection, QUERY_SCHEMA_VERSION, source_revision,
};
use super::validation_json::{json_str, span_json};

pub(super) fn graph_id(module_display: &str, f: &AST::Func) -> String {
    let file = Path::new(module_display)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(module_display);
    format!(
        "fn:{file}::{}@{}-{}",
        f.name, f.name_span.start, f.name_span.end
    )
}

pub(super) fn span_through_closing_parens(src: &str, span: Span) -> SourceSpan {
    let Some(text) = src.get(span.start..span.end) else {
        return span.into();
    };
    let opens = text.bytes().filter(|byte| *byte == b'(').count();
    let mut closes = text.bytes().filter(|byte| *byte == b')').count();
    let mut end = span.end;
    while closes < opens && src.as_bytes().get(end) == Some(&b')') {
        end += 1;
        closes += 1;
    }
    SourceSpan {
        start: span.start,
        end,
    }
}

pub(super) fn graph_id_name_span(graph_id: &str) -> Option<SourceSpan> {
    let (_, range) = graph_id.rsplit_once('@')?;
    let (start, end) = range.split_once('-')?;
    Some(SourceSpan {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    })
}

pub(super) fn function_signature_span(src: &str, name_span: SourceSpan) -> Result<SourceSpan, String> {
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

pub(super) fn insert_offset(src: &str, f: &AST::Func) -> usize {
    if let Some(first) = f.body.first() {
        let first_start = first.span().start;
        let line = line_start(src, first_start);
        if src[line..first_start].contains('{') {
            return first_start;
        }
        return line;
    }
    line_after(src, f.name_span.end)
}

pub(super) fn line_start(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

pub(super) fn line_after(src: &str, offset: usize) -> usize {
    src[offset.min(src.len())..]
        .find('\n')
        .map(|i| offset + i + 1)
        .unwrap_or(src.len())
}

pub(super) fn indentation_at(src: &str, offset: usize) -> String {
    let start = line_start(src, offset);
    src[start..offset.min(src.len())]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

pub(super) fn assignment_title(target: &AST::LValue, op: Option<AST::BinOp>) -> String {
    let op = op
        .map(|op| format!("{op:?}="))
        .unwrap_or_else(|| "=".to_string());
    match target {
        AST::LValue::Local { name, .. } => format!("{name} {op}"),
        AST::LValue::Index { .. } => format!("index {op}"),
        AST::LValue::Field { field, .. } => format!(".{field} {op}"),
    }
}

pub(super) fn lvalue_type(g: &GraphBuilder, target: &AST::LValue) -> String {
    match target {
        AST::LValue::Local { name, .. } => g
            .local_types
            .get(name)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        AST::LValue::Index { .. } | AST::LValue::Field { .. } => "unknown".to_string(),
    }
}

pub(super) fn expr_title(expr: &Expr) -> &'static str {
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
        Expr::Deref(_, _) | Expr::RawOf(_, _) | Expr::PtrFromAddr { .. } => "unsafe expr",
        Expr::OrFallback { .. } => "fallback",
        Expr::PatternTest { .. } => "pattern test",
        Expr::Try(_, _, _) => "fallible",
        _ => "expression",
    }
}

pub(super) fn starts_uppercase(s: &str) -> bool {
    s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}

pub(super) fn binding_type(g: &GraphBuilder, name: &str, b: &AST::Binding) -> String {
    b.ty.as_ref()
        .map(AST::Type::name)
        .or_else(|| g.local_types.get(name).cloned())
        .unwrap_or_else(|| "unknown".to_string())
}

pub(super) fn expr_type(g: &GraphBuilder, index: &SemIndex, expr: &Expr) -> String {
    match expr {
        Expr::Int(_, _, _, _) => "Int".to_string(),
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

pub(super) fn call_ret(index: &SemIndex, name: &str) -> Option<String> {
    index.definitions().iter().find_map(|d| {
        if d.name == name {
            if let SymbolKind::Function { ret, .. } = &d.kind {
                return ret.clone();
            }
        }
        None
    })
}

pub(super) fn effect_badges(index: &SemIndex, function: &str) -> Vec<&'static str> {
    if let Some(effects) = index.effect_of(function) {
        if !effects.direct.is_empty() || !effects.inferred.is_empty() {
            return vec!["effects"];
        }
    }
    Vec::new()
}

pub(super) fn call_has_effects(index: &SemIndex, function: &str) -> bool {
    index
        .effect_of(function)
        .map(|effects| !effects.direct.is_empty() || !effects.inferred.is_empty())
        .unwrap_or(false)
}

pub(super) fn pure_leaf(expr: &Expr) -> bool {
    match expr {
        Expr::Str(_, _)
        | Expr::Int(_, _, _, _)
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
        Expr::ListLit(_, _) => false,
        Expr::MapLit(items, _) => items.iter().all(|(k, v)| pure_leaf(k) && pure_leaf(v)),
        Expr::TupleLit(items, _, _) => items.iter().all(|(_, e)| pure_leaf(e)),
        Expr::Ok(inner, _) | Expr::Err(inner, _) | Expr::Present(inner, _) => pure_leaf(inner),
        _ => false,
    }
}

pub(super) fn wire_ident_refs(g: &mut GraphBuilder, expr: &Expr, input_pin: &str) {
    if let Expr::Ident(name, span) = expr {
        if let Some(out) = g.local_pins.get(name).cloned() {
            add_wire_with_span(g, &out, input_pin, "data", Some((*span).into()));
        }
    }
}

pub(super) fn snippet(src: &str, span: Span) -> String {
    src.get(span.start..span.end)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub(super) fn edit(span: SourceSpan, text: &str) -> TextEdit {
    TextEdit {
        span: Span::new(span.start, span.end),
        new_text: text.to_string(),
    }
}

pub(super) fn edit_ok(changed: bool, path: &Path) -> String {
    let src = fs::read_to_string(path).unwrap_or_default();
    format!(
        "{{\"protocol\":\"jet.canvas.edit\",\"schema_version\":{},\"changed\":{},\"revision\":{},\"source_text\":{}}}",
        EDIT_SCHEMA_VERSION,
        if changed { "true" } else { "false" },
        json_str(&source_revision(&src)),
        json_str(&src)
    )
}

pub(super) fn preview_ok(before: &str, after: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.preview\",\"schema_version\":{},\"changed\":{},\"diff\":{},\"after_revision\":{}}}",
        EDIT_SCHEMA_VERSION,
        if before != after { "true" } else { "false" },
        json_str(&simple_diff(before, after)),
        json_str(&source_revision(after))
    )
}

pub(super) fn canvas_action_preview_ok(
    path: &Path,
    before: &str,
    after: &str,
    action_id: &str,
    callee: &str,
) -> String {
    let authority = canvas_authority_context(path);
    format!(
        "{{\"protocol\":\"jet.canvas.action\",\"schema_version\":{},\"ok\":true,\"changed\":{},\"action_id\":{},\"callee\":{},\"engine\":\"checked-tir+jit\",\"execution\":\"preview\",\"writes\":\"source_transaction_only\",\"authority\":[{}],\"audit\":{{\"package_id\":{},\"version\":{},\"hash\":{},\"touched_files\":[{}],\"diagnostics\":[]}},\"diff\":{},\"after_revision\":{}}}",
        ACTION_SCHEMA_VERSION,
        if before != after { "true" } else { "false" },
        json_str(action_id),
        json_str(callee),
        json_str(&authority.grant),
        json_str(&authority.package_id),
        json_str(&authority.version),
        json_str(&source_revision(after)),
        json_str(&authority.touched_file),
        json_str(&simple_diff(before, after)),
        json_str(&source_revision(after))
    )
}

pub(super) fn query_ok(op: &str, src: &str, results_json: &str, extra_fields: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.query\",\"schema_version\":{},\"ok\":true,\"op\":{},\"revision\":{},\"results\":{},{} }}",
        QUERY_SCHEMA_VERSION,
        json_str(op),
        json_str(&source_revision(src)),
        results_json,
        extra_fields
    )
}

pub(super) fn query_result_json(
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

pub(super) fn open_query_context(path: &Path, src: &str) -> Result<(Projection, SemIndex), String> {
    let projection =
        project_file(path).map_err(|diags| query_diagnostics_error(path, src, &diags))?;
    let index = jet_semindex::open(path).map_err(|e| query_error("check", &e.to_string()))?;
    Ok((projection, index))
}

pub(super) fn node_for_span(projection: &Projection, span: SourceSpan) -> Option<NodeQueryRef> {
    projection
        .node_refs
        .iter()
        .filter(|node| spans_overlap(node.span, span))
        .min_by_key(|node| node.span.end.saturating_sub(node.span.start))
        .cloned()
}

pub(super) fn nearest_node(projection: &Projection, offset: usize) -> Option<NodeQueryRef> {
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

pub(super) fn spans_overlap(a: SourceSpan, b: SourceSpan) -> bool {
    a.start <= b.end && b.start <= a.end
}

pub(super) fn contains_ci(text: &str, needle: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

pub(super) fn text_matches(src: &str, needle: &str) -> Vec<SourceSpan> {
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

pub(super) fn dedupe_results(results: &mut Vec<String>) {
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

pub(super) fn simple_diff(before: &str, after: &str) -> String {
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

pub(super) fn edit_error(kind: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.edit\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        EDIT_SCHEMA_VERSION,
        json_str(kind),
        json_str(message)
    )
}

fn edit_error_with_diagnostics(
    kind: &str,
    message: &str,
    path: &Path,
    src: &str,
    diags: &[Diagnostic],
) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.edit\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{},\"revision\":{},\"diagnostic_revision\":{},\"diagnostics\":[{}]}}",
        EDIT_SCHEMA_VERSION,
        json_str(kind),
        json_str(message),
        json_str(&source_revision(src)),
        json_str(&source_revision(src)),
        diagnostics_json(path, src, diags)
    )
}

pub(super) fn query_error(kind: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.query\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        QUERY_SCHEMA_VERSION,
        json_str(kind),
        json_str(message)
    )
}

fn query_error_with_diagnostics(
    kind: &str,
    message: &str,
    path: &Path,
    src: &str,
    diags: &[Diagnostic],
) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.query\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{},\"revision\":{},\"diagnostic_revision\":{},\"diagnostics\":[{}]}}",
        QUERY_SCHEMA_VERSION,
        json_str(kind),
        json_str(message),
        json_str(&source_revision(src)),
        json_str(&source_revision(src)),
        diagnostics_json(path, src, diags)
    )
}

pub(super) fn project_edit_ok(
    op: &str,
    preview: bool,
    changed: bool,
    before_revision: &str,
    after_revision: &str,
    touched_files: &str,
    diff: &str,
) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.project.edit\",\"schema_version\":{},\"ok\":true,\"op\":{},\"preview\":{},\"changed\":{},\"project_revision\":{},\"after_project_revision\":{},\"writes\":{},\"authority\":[\"canvas.source_edit:project\"],\"audit\":{{\"touched_files\":[{}],\"diagnostics\":[]}},\"diff\":{}}}",
        PROJECT_SCHEMA_VERSION,
        json_str(op),
        if preview { "true" } else { "false" },
        if changed { "true" } else { "false" },
        json_str(before_revision),
        json_str(after_revision),
        if preview { "\"preview_only\"" } else { "\"source_transaction\"" },
        touched_files,
        json_str(diff)
    )
}

pub(super) fn project_edit_error(kind: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.project.edit\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        PROJECT_SCHEMA_VERSION,
        json_str(kind),
        json_str(message)
    )
}

pub(super) fn diagnostics_error(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    edit_error_with_diagnostics(
        "diagnostic",
        &jet_driver::Diagnostics::render_all(&path.display().to_string(), src, diags),
        path,
        src,
        diags,
    )
}

pub(super) fn query_diagnostics_error(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    query_error_with_diagnostics(
        "diagnostic",
        &jet_driver::Diagnostics::render_all(&path.display().to_string(), src, diags),
        path,
        src,
        diags,
    )
}

pub(super) fn diagnostics_json(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    diags
        .iter()
        .map(|d| diagnostic_payload_json(path, src, d))
        .collect::<Vec<_>>()
        .join(",")
}

fn diagnostic_payload_json(path: &Path, src: &str, d: &Diagnostic) -> String {
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Lint => "warning",
    };
    let span_json = match d.span {
        Some(span) => {
            let (line, column) = jet_driver::Diagnostics::span_line_col(src, span.start);
            format!(
                "{{\"start\":{},\"end\":{},\"line\":{},\"column\":{}}}",
                span.start, span.end, line, column
            )
        }
        None => "null".to_string(),
    };
    let rendered = jet_driver::Diagnostics::render_all(&path.display().to_string(), src, std::slice::from_ref(d));
    format!(
        "{{\"code\":{},\"severity\":{},\"what\":{},\"why\":{},\"fix\":{},\"message\":{},\"rendered\":{},\"source_span\":{},\"source_path\":{}}}",
        json_str(&d.code),
        json_str(severity),
        json_str(&d.what),
        json_str(&d.why),
        json_str(&d.fix),
        json_str(&d.what),
        json_str(&rendered),
        span_json,
        json_str(&path.display().to_string())
    )
}
