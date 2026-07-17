//! JSON-RPC transport over stdio + request/notification dispatch + handlers.

use crate::Diagnostics::{Diagnostic, Severity, Span};
use crate::Lexer::{TokKind, Token};
use crate::AST::ProgramBundle;
use jet_queries::{InputKey, QueryEngine, QueryKey};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

use super::Check::{check_document_with_bundle, collect_fixes, Fix};
use super::Completion::compute_completions;
use super::Features::{
    compute_definition, compute_generated_definition, compute_hover, compute_references,
    compute_rename, encode_semantic_tokens, encode_semantic_tokens_in_span, format_inlay_hints,
};
use super::Position::{
    apply_lsp_edit, byte_offset_to_lsp, byte_span_to_range, full_document_range, lsp_pos_to_offset,
    range_json, LspPos, LspRange,
};
use super::SymbolDB::{build_symbol_db, InlayHint, SymKind, SymbolDB};
use jet_foundation::JSON::{json_escape, json_get, json_int, json_str, parse_json, JsonValue};

// ── Document state ────────────────────────────────────────────────────────────

struct Document {
    path: String,
    text: String,
}

#[derive(Clone)]
struct CheckedBundle {
    diags: Vec<Diagnostic>,
    bundle: Option<Arc<ProgramBundle>>,
    facts: jet_semindex::SemIndexEffectFacts,
}

impl Document {
    fn new(path: String, text: String) -> Self {
        Document { path, text }
    }

    fn replace_text(&mut self, text: String) {
        self.text = text;
    }

    fn apply_range_edit(&mut self, range: LspRange, text: &str) {
        self.text = apply_lsp_edit(&self.text, range, text);
    }
}

struct Server {
    docs: HashMap<String, Document>,
    workspace_roots: Vec<String>,
    /// URIs of documents that changed since last diagnostic publish (D-LSP3).
    dirty: std::collections::HashSet<String>,
    /// D-LSP1 stage 1: shared query engine for memoized document facts.
    queries: std::cell::RefCell<QueryEngine>,
    shutdown: bool,
}

impl Server {
    fn new() -> Self {
        Server {
            docs: HashMap::new(),
            workspace_roots: Vec::new(),
            dirty: std::collections::HashSet::new(),
            queries: std::cell::RefCell::new(QueryEngine::new()),
            shutdown: false,
        }
    }

    fn sync_doc_input(&self, doc: &Document) {
        self.queries
            .borrow_mut()
            .set_input(InputKey::new(doc.path.clone()), doc.text.clone());
    }

    /// D-LSP1 stage 2: diagnostics run through the query engine instead of a
    /// private LSP-only cache.
    fn check(&self, doc: &Document) -> Vec<Diagnostic> {
        self.check_with_bundle(doc).diags
    }

    fn check_with_bundle(&self, doc: &Document) -> CheckedBundle {
        let path = doc.path.clone();
        self.sync_doc_input(doc);
        self.queries.borrow_mut().query(
            QueryKey::new("lsp.checked_bundle", path.clone()),
            |queries| {
                let text = queries
                    .input_text(&InputKey::new(path.clone()))
                    .unwrap_or_default();
                let (diags, bundle, facts) = check_document_with_bundle(&path, &text);
                CheckedBundle {
                    diags,
                    bundle: bundle.map(Arc::new),
                    facts,
                }
            },
        )
    }

    fn lex(&self, doc: &Document) -> Arc<Vec<Token>> {
        let path = doc.path.clone();
        self.sync_doc_input(doc);
        self.queries
            .borrow_mut()
            .query(QueryKey::new("lsp.tokens", path.clone()), |queries| {
                let text = queries.input_text(&InputKey::new(path)).unwrap_or_default();
                let (toks, _) = crate::Lexer::lex(&text);
                Arc::new(toks)
            })
    }

    fn fixes(&self, doc: &Document) -> Vec<Fix> {
        let path = doc.path.clone();
        self.sync_doc_input(doc);
        self.queries
            .borrow_mut()
            .query(QueryKey::new("lsp.fixes", path.clone()), |queries| {
                let text = queries
                    .input_text(&InputKey::new(path.clone()))
                    .unwrap_or_default();
                collect_fixes(&path, &text)
            })
    }
}

// ── JSON-RPC main loop ────────────────────────────────────────────────────────

pub fn run_stdio() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout();
    let mut server = Server::new();

    loop {
        let body = match read_message(&mut stdin)? {
            Some(b) => b,
            None => break,
        };
        let msg = match parse_json(&body) {
            Ok(v) => v,
            Err(()) => continue,
        };
        let method = json_get(&msg, "method").and_then(json_str);
        let id = json_get(&msg, "id").cloned();
        let params = json_get(&msg, "params");

        if let Some(method) = method {
            if id.is_some() {
                // D-LSP3: flush any buffered dirty-document diagnostics before serving requests.
                let _ = flush_dirty(&mut server, &mut stdout);
                let resp = catch_handler(std::panic::AssertUnwindSafe(|| {
                    handle_request(&mut server, method, params, id.as_ref().unwrap())
                }));
                if let Some(resp) = resp {
                    write_message(&mut stdout, &resp)?;
                }
            } else {
                catch_notification(|| {
                    handle_notification(&mut server, method, params, &mut stdout)
                })?;
            }
        }

        if server.shutdown {
            break;
        }
    }
    Ok(())
}

/// Catch panics in a handler; on panic, log and return None (LSP-I2).
fn catch_handler<F: FnOnce() -> Option<String>>(
    f: std::panic::AssertUnwindSafe<F>,
) -> Option<String> {
    match std::panic::catch_unwind(f) {
        Ok(r) => r,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            let _ = write_log(&format!("ICE in handler: {}", msg));
            None
        }
    }
}

/// Catch panics in a notification handler (LSP-I2).
fn catch_notification<F: FnOnce() -> io::Result<()>>(f: F) -> io::Result<()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Ok(()),
    }
}

fn write_log(msg: &str) -> io::Result<()> {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/jet-lsp.log")
    {
        writeln!(f, "[jet-lsp] {}", msg)?;
    }
    Ok(())
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
    }
    let len = match content_length {
        Some(l) => l,
        None => return Ok(None),
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

fn write_message<W: Write>(w: &mut W, json: &str) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n{}", json.len(), json)?;
    w.flush()
}

// ── Request handlers ──────────────────────────────────────────────────────────

fn handle_request(
    server: &mut Server,
    method: &str,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    // c121 Step 5: record per-request latency when JET_TIMING=1. Off by
    // default, so the normal LSP path pays nothing.
    let started = if crate::PhaseTiming::enabled() {
        Some(std::time::Instant::now())
    } else {
        None
    };
    let out = match method {
        "initialize" => {
            configure_workspace_roots(server, params);
            Some(initialize_response(id))
        }
        "shutdown" => {
            server.shutdown = true;
            Some(response(id, "null"))
        }
        "textDocument/codeAction" => code_action_response(server, params, id),
        "textDocument/formatting" => format_response(server, params, id),
        "textDocument/rangeFormatting" => format_response(server, params, id),
        "textDocument/completion" => completion_response(server, params, id),
        "textDocument/signatureHelp" => signature_help_response(server, params, id),
        "textDocument/documentSymbol" => document_symbol_response(server, params, id),
        "textDocument/foldingRange" => folding_range_response(server, params, id),
        "textDocument/documentHighlight" => document_highlight_response(server, params, id),
        "textDocument/selectionRange" => selection_range_response(server, params, id),
        "textDocument/documentLink" => document_link_response(server, params, id),
        "textDocument/codeLens" => code_lens_response(server, params, id),
        "textDocument/hover" => hover_response(server, params, id),
        "textDocument/definition" => definition_response(server, params, id),
        "textDocument/references" => references_response(server, params, id),
        "textDocument/prepareRename" => prepare_rename_response(server, params, id),
        "textDocument/rename" => rename_response(server, params, id),
        "textDocument/semanticTokens/full" => semantic_tokens_response(server, params, id),
        "textDocument/semanticTokens/range" => semantic_tokens_range_response(server, params, id),
        "textDocument/semanticTokens/full/delta" => {
            semantic_tokens_delta_response(server, params, id)
        }
        "textDocument/inlayHint" => inlay_hint_response(server, params, id),
        "workspace/symbol" => workspace_symbol_response(server, params, id),
        "textDocument/prepareCallHierarchy" => prepare_call_hierarchy_response(server, params, id),
        "callHierarchy/incomingCalls" => call_hierarchy_incoming_response(server, params, id),
        "callHierarchy/outgoingCalls" => call_hierarchy_outgoing_response(server, params, id),
        "textDocument/prepareTypeHierarchy" => prepare_type_hierarchy_response(server, params, id),
        "typeHierarchy/supertypes" => type_hierarchy_supertypes_response(server, params, id),
        "typeHierarchy/subtypes" => type_hierarchy_subtypes_response(server, params, id),
        "workspace/executeCommand" => execute_command_response(server, params, id),
        "jet/buildGraph" => build_graph_response(server, params, id),
        _ => Some(response(id, "null")),
    };
    if let Some(t0) = started {
        record_lsp_latency(method, t0.elapsed().as_micros());
    }
    out
}

fn build_graph_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let uri = params
        .and_then(|params| json_get(params, "textDocument"))
        .and_then(|document| json_get(document, "uri"))
        .and_then(json_str)?;
    let document = server.docs.get(uri)?;
    let graph = match super::Check::build_graph_json(&document.path, &document.text) {
        Ok(Some(graph)) => graph,
        Ok(None) => return Some(response(id, "null")),
        Err(_) => return Some(response(id, "null")),
    };
    Some(response(id, &graph))
}

/// c121 Step 5: append one `{"method":…,"us":…}` JSON line to
/// `jet-lsp-timing.json` in the cwd. JSON-lines suits an open-ended request
/// stream; best-effort, so a write failure never disturbs the server.
fn record_lsp_latency(method: &str, us: u128) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("jet-lsp-timing.json")
    {
        let _ = writeln!(f, "{{\"method\":\"{}\",\"us\":{}}}", method, us);
    }
}

fn configure_workspace_roots(server: &mut Server, params: Option<&JsonValue>) {
    let mut roots = Vec::new();
    if let Some(params) = params {
        if let Some(JsonValue::Array(folders)) = json_get(params, "workspaceFolders") {
            for folder in folders {
                if let Some(uri) = json_get(folder, "uri").and_then(json_str) {
                    push_workspace_root(&mut roots, uri_to_path(uri));
                }
            }
        }
        if let Some(uri) = json_get(params, "rootUri").and_then(json_str) {
            push_workspace_root(&mut roots, uri_to_path(uri));
        }
        if let Some(path) = json_get(params, "rootPath").and_then(json_str) {
            push_workspace_root(&mut roots, path.to_string());
        }
    }
    server.workspace_roots = roots;
}

fn push_workspace_root(roots: &mut Vec<String>, path: String) {
    let path = normalize_path(&path);
    if !path.is_empty() && !roots.iter().any(|root| root == &path) {
        roots.push(path);
    }
}

fn handle_notification(
    server: &mut Server,
    method: &str,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    match method {
        "initialized" => Ok(()),
        "exit" => {
            server.shutdown = true;
            Ok(())
        }
        "textDocument/didOpen" => publish_after_open(server, params, stdout),
        "textDocument/didChange" => publish_after_change(server, params, stdout),
        "textDocument/didClose" => {
            if let Some(uri) = params
                .and_then(|p| json_get(p, "textDocument"))
                .and_then(|td| json_get(td, "uri"))
                .and_then(json_str)
            {
                if let Some(doc) = server.docs.remove(uri) {
                    server
                        .queries
                        .borrow_mut()
                        .remove_input(&InputKey::new(doc.path));
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn initialize_response(id: &JsonValue) -> String {
    let result = r#"{
  "capabilities": {
    "textDocumentSync": 2,
    "documentFormattingProvider": true,
    "documentRangeFormattingProvider": true,
    "codeActionProvider": true,
    "completionProvider": {
      "triggerCharacters": ["."],
      "resolveProvider": false
    },
    "signatureHelpProvider": {
      "triggerCharacters": ["(", ","]
    },
    "documentSymbolProvider": true,
    "workspaceSymbolProvider": true,
    "foldingRangeProvider": true,
    "documentHighlightProvider": true,
    "selectionRangeProvider": true,
    "documentLinkProvider": { "resolveProvider": false },
    "codeLensProvider": { "resolveProvider": false },
    "hoverProvider": true,
    "definitionProvider": true,
    "referencesProvider": true,
    "renameProvider": {
      "prepareProvider": true
    },
    "semanticTokensProvider": {
      "legend": {
        "tokenTypes": [
          "keyword","type","function","variable","parameter",
          "property","enumMember","string","number","comment",
          "operator","namespace","ownership","decorator"
        ],
        "tokenModifiers": [
          "declaration","readonly","move","writeBorrow","copy",
          "rule"
        ]
      },
      "full": { "delta": true },
      "range": true
    },
    "inlayHintProvider": true,
    "callHierarchyProvider": true,
    "typeHierarchyProvider": true,
    "executeCommandProvider": {
      "commands": ["jet.impact", "jet.budgetReports"]
    }
  },
  "serverInfo": { "name": "jet", "version": "0.2.0" }
}"#;
    response(id, result)
}

fn response(id: &JsonValue, result_json: &str) -> String {
    let id_json = serialize_id(id);
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"result":{}}}"#,
        id_json, result_json
    )
}

fn error_response(id: &JsonValue, code: i64, message: &str) -> String {
    let id_json = serialize_id(id);
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":{},"message":"{}"}}}}"#,
        id_json,
        code,
        json_escape(message)
    )
}

fn serialize_id(id: &JsonValue) -> String {
    match id {
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => format!("\"{}\"", json_escape(s)),
        _ => "null".to_string(),
    }
}

fn publish_after_change(
    server: &mut Server,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    publish_after_change_impl(server, params, stdout, false)
}

fn publish_after_open(
    server: &mut Server,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    publish_after_change_impl(server, params, stdout, true)
}

/// D-LSP3: on `didChange` (is_open=false), mark dirty but don't publish immediately.
/// On `didOpen` (is_open=true), always publish so the editor gets initial diagnostics.
/// Dirty documents are flushed before the next request that reads document state.
fn publish_after_change_impl(
    server: &mut Server,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
    is_open: bool,
) -> io::Result<()> {
    let params = match params {
        Some(p) => p,
        None => return Ok(()),
    };
    let td = match json_get(params, "textDocument") {
        Some(v) => v,
        None => return Ok(()),
    };
    let uri = match json_get(td, "uri").and_then(json_str) {
        Some(u) => u.to_string(),
        None => return Ok(()),
    };
    let path = uri_to_path(&uri);

    if let Some(text) = json_get(td, "text").and_then(json_str) {
        server
            .docs
            .insert(uri.clone(), Document::new(path, text.to_string()));
    } else if let Some(changes) = json_get(params, "contentChanges") {
        if let JsonValue::Array(arr) = changes {
            let doc = server
                .docs
                .entry(uri.clone())
                .or_insert_with(|| Document::new(path, String::new()));
            for change in arr {
                apply_content_change(doc, change);
            }
        }
    }
    if let Some(doc) = server.docs.get(&uri) {
        server.sync_doc_input(doc);
    }

    if is_open {
        // Always publish on open — client expects initial diagnostics.
        if let Some(doc) = server.docs.get(&uri) {
            let diags = server.check(doc);
            let notif = publish_diagnostics(&uri, &doc.text, &diags);
            write_message(stdout, &notif)?;
        }
        server.dirty.remove(&uri);
    } else {
        // Mark dirty; diagnostics will be flushed before the next request.
        server.dirty.insert(uri);
    }
    Ok(())
}

fn apply_content_change(doc: &mut Document, change: &JsonValue) {
    let obj = match change {
        JsonValue::Object(obj) => obj,
        _ => return,
    };
    let text = match obj.get("text").and_then(json_str) {
        Some(text) => text,
        None => return,
    };
    if let Some(range) = obj.get("range").and_then(range_from_json) {
        doc.apply_range_edit(range, text);
    } else {
        doc.replace_text(text.to_string());
    }
}

fn range_from_json(value: &JsonValue) -> Option<LspRange> {
    let start = json_get(value, "start")?;
    let end = json_get(value, "end")?;
    Some(LspRange {
        start: pos_from_json(start)?,
        end: pos_from_json(end)?,
    })
}

fn pos_from_json(value: &JsonValue) -> Option<LspPos> {
    Some(LspPos {
        line: json_int(json_get(value, "line")?)? as u32,
        character: json_int(json_get(value, "character")?)? as u32,
    })
}

/// Flush any pending dirty-document diagnostics before handling a request (D-LSP3).
fn flush_dirty(server: &mut Server, stdout: &mut impl Write) -> io::Result<()> {
    let dirty: Vec<String> = server.dirty.drain().collect();
    for uri in dirty {
        if let Some(doc) = server.docs.get(&uri) {
            let text = doc.text.clone();
            let diags = server.check(doc);
            let notif = publish_diagnostics(&uri, &text, &diags);
            write_message(stdout, &notif)?;
        }
    }
    Ok(())
}

fn publish_diagnostics(uri: &str, src: &str, diags: &[Diagnostic]) -> String {
    let mut items = String::new();
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        items.push_str(&diagnostic_json(d, src));
    }
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":[{}]}}}}"#,
        json_escape(uri),
        items
    )
}

fn diagnostic_json(d: &Diagnostic, src: &str) -> String {
    let severity = match d.severity {
        Severity::Error => 1,
        Severity::Lint => 2,
    };
    let range = d
        .span
        .map(|s| byte_span_to_range(src, s))
        .unwrap_or(full_document_range(src));
    format!(
        r#"{{"range":{},"severity":{},"code":"{}","source":"jet","message":"{}"}}"#,
        range_json(range),
        severity,
        json_escape(&d.code),
        json_escape(&d.what)
    )
}

fn code_action_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    // Go through the SAME unified fix engine the CLI `jet fix` uses, so a fix
    // offered in the editor is byte-identical to a fix applied on the command
    // line.
    let fixes = server.fixes(doc);
    let mut actions = String::new();
    for (n, fix) in fixes.iter().enumerate() {
        if n > 0 {
            actions.push(',');
        }
        actions.push_str(&code_action_json(uri, &doc.text, fix));
    }
    Some(response(id, &format!("[{}]", actions)))
}

fn code_action_json(uri: &str, src: &str, fix: &Fix) -> String {
    let range = byte_span_to_range(src, fix.edit.span);
    format!(
        r#"{{"title":"{}","kind":"quickfix","edit":{{"changes":{{"{}":[{{"range":{},"newText":"{}"}}]}}}}}}"#,
        json_escape(&fix.title),
        json_escape(uri),
        range_json(range),
        json_escape(&fix.edit.new_text)
    )
}

fn format_response(server: &Server, params: Option<&JsonValue>, id: &JsonValue) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let formatted = match crate::format_source(&doc.text) {
        Ok(s) => s,
        Err(_) => return Some(response(id, "[]")),
    };
    let range = full_document_range(&doc.text);
    let edit = format!(
        r#"[{{"range":{},"newText":"{}"}}]"#,
        range_json(range),
        json_escape(&formatted)
    );
    Some(response(id, &edit))
}

fn completion_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let checked = server.check_with_bundle(doc);
    let mut db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };
    merge_workspace_defs(server, doc, &mut db);

    let discovery = load_discovery_index(&doc.path);
    let workspace_root = workspace_root_for_path(server, &doc.path);
    let items = compute_completions(
        &db,
        &doc.text,
        offset,
        &doc.path,
        workspace_root.as_deref(),
        discovery.as_ref(),
    );
    let mut json_items = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            json_items.push(',');
        }
        json_items.push_str(&item.to_json());
    }
    Some(response(
        id,
        &format!(r#"{{"isIncomplete":false,"items":[{}]}}"#, json_items),
    ))
}

fn signature_help_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let offset = lsp_pos_to_offset(&doc.text, LspPos { line, character });
    let call = match active_call(&doc.text, offset) {
        Some(call) => call,
        None => return Some(response(id, "null")),
    };

    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    let def = match db
        .defs
        .iter()
        .find(|def| def.name == call.name && matches!(def.kind, SymKind::Function { .. }))
    {
        Some(def) => def,
        None => return Some(response(id, "null")),
    };

    let (label, params_json, param_count) = match &def.kind {
        SymKind::Function { params, ret, effects } => {
            let parts: Vec<String> = params
                .iter()
                .map(|(name, ty)| format!("{}: {}", name, ty.name()))
                .collect();
            let mut label = format!("fn {}({})", def.name, parts.join(", "));
            if let Some(effects) = effects {
                label.push_str(" --[");
                label.push_str(&effects.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", "));
                label.push_str("]->");
            } else if ret.is_some() {
                label.push_str(" ->");
            }
            if let Some(ret) = ret {
                label.push_str(&format!(" {}", ret.name()));
            }
            let params_json = parts
                .iter()
                .map(|part| format!(r#"{{"label":"{}"}}"#, json_escape(part)))
                .collect::<Vec<_>>()
                .join(",");
            (label, params_json, parts.len())
        }
        _ => return Some(response(id, "null")),
    };
    let active = if param_count == 0 {
        0
    } else {
        call.active_param.min(param_count.saturating_sub(1))
    };
    let result = format!(
        r#"{{"signatures":[{{"label":"{}","parameters":[{}]}}],"activeSignature":0,"activeParameter":{}}}"#,
        json_escape(&label),
        params_json,
        active
    );
    Some(response(id, &result))
}

fn document_symbol_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    let mut defs = db
        .defs
        .iter()
        .filter(|def| def.module_path == doc.path)
        .filter_map(|def| document_symbol_kind(&def.kind).map(|kind| (def, kind)))
        .collect::<Vec<_>>();
    defs.sort_by_key(|(def, _)| def.def_span.start);

    let mut items = String::new();
    for (i, (def, kind)) in defs.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        let range = range_json(byte_span_to_range(&doc.text, def.def_span));
        items.push_str(&format!(
            r#"{{"name":"{}","kind":{},"range":{},"selectionRange":{}}}"#,
            json_escape(&def.name),
            kind,
            range,
            range
        ));
    }
    Some(response(id, &format!("[{}]", items)))
}

fn workspace_symbol_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let query = params
        .and_then(|p| json_get(p, "query"))
        .and_then(json_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut items = String::new();
    let mut first = true;
    let root_hint = server.docs.values().next().map(|doc| doc.path.as_str());
    let mut docs = workspace_sources(server, root_hint);
    docs.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, text) in docs {
        let db = if let Some(doc) = server.docs.values().find(|doc| doc.path == path) {
            let checked = server.check_with_bundle(doc);
            match checked.bundle {
                Some(b) => build_symbol_db(&b, &checked.facts),
                None => SymbolDB::new(),
            }
        } else {
            let (_diags, bundle, facts) = check_document_with_bundle(&path, &text);
            match bundle {
                Some(b) => build_symbol_db(&b, &facts),
                None => SymbolDB::new(),
            }
        };
        let mut defs = db
            .defs
            .iter()
            .filter(|def| def.module_path == path)
            .filter_map(|def| document_symbol_kind(&def.kind).map(|kind| (def, kind)))
            .filter(|(def, _)| query.is_empty() || def.name.to_ascii_lowercase().contains(&query))
            .collect::<Vec<_>>();
        defs.sort_by_key(|(def, _)| def.def_span.start);
        for (def, kind) in defs {
            if !first {
                items.push(',');
            }
            first = false;
            let uri = path_to_uri(&path);
            let range = range_json(byte_span_to_range(&text, def.def_span));
            items.push_str(&format!(
                r#"{{"name":"{}","kind":{},"location":{{"uri":"{}","range":{}}}}}"#,
                json_escape(&def.name),
                kind,
                json_escape(&uri),
                range
            ));
        }
    }
    Some(response(id, &format!("[{}]", items)))
}

fn folding_range_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let folds = compute_folding_ranges(&doc.text);
    let mut out = String::new();
    for (i, fold) in folds.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&fold.to_json());
    }
    Some(response(id, &format!("[{}]", out)))
}

fn document_highlight_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let offset = lsp_pos_to_offset(&doc.text, LspPos { line, character });

    let tokens = server.lex(doc);
    let name = match ident_at(&tokens, offset) {
        Some(name) => name.to_string(),
        None => return Some(response(id, "[]")),
    };
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    let mut spans: Vec<(Span, u8)> = Vec::new();
    for def in db
        .defs
        .iter()
        .filter(|def| def.module_path == doc.path && def.name == name)
    {
        spans.push((def.def_span, 3));
    }
    for r in db
        .refs
        .iter()
        .filter(|r| r.module_path == doc.path && r.name == name)
    {
        spans.push((r.span, 2));
    }
    spans.sort_by_key(|(span, kind)| (span.start, *kind));
    spans.dedup_by_key(|(span, kind)| (span.start, span.end, *kind));

    let mut out = String::new();
    for (i, (span, kind)) in spans.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            r#"{{"range":{},"kind":{}}}"#,
            range_json(byte_span_to_range(&doc.text, *span)),
            kind
        ));
    }
    Some(response(id, &format!("[{}]", out)))
}

fn selection_range_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let positions = match json_get(params, "positions") {
        Some(JsonValue::Array(values)) => values,
        _ => return Some(response(id, "[]")),
    };
    let tokens = server.lex(doc);
    let blocks = brace_block_spans(&doc.text);

    let mut out = String::new();
    for (i, pos) in positions.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let line = match json_get(pos, "line").and_then(json_int) {
            Some(line) => line as u32,
            None => continue,
        };
        let character = match json_get(pos, "character").and_then(json_int) {
            Some(character) => character as u32,
            None => continue,
        };
        let offset = lsp_pos_to_offset(&doc.text, LspPos { line, character });
        let ranges = selection_ranges_for(&doc.text, &tokens, &blocks, offset);
        out.push_str(&selection_range_json(&ranges));
    }
    Some(response(id, &format!("[{}]", out)))
}

fn document_link_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let workspace_root = workspace_root_for_path(server, &doc.path);
    let links = document_links_for(&doc.path, workspace_root.as_deref(), &doc.text);
    Some(response(id, &format!("[{}]", links.join(","))))
}

fn code_lens_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let lenses = code_lenses_for(uri, &doc.text);
    Some(response(id, &format!("[{}]", lenses.join(","))))
}

fn prepare_call_hierarchy_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let offset = lsp_pos_to_offset(&doc.text, LspPos { line, character });
    let tokens = server.lex(doc);
    let name = match ident_at(&tokens, offset) {
        Some(name) => name,
        None => return Some(response(id, "[]")),
    };
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };
    let item = db
        .defs
        .iter()
        .find(|def| def.module_path == doc.path && def.name == name)
        .and_then(|def| call_hierarchy_item_json(def, &doc.text));
    match item {
        Some(item) => Some(response(id, &format!("[{}]", item))),
        None => Some(response(id, "[]")),
    }
}

fn call_hierarchy_incoming_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let item = json_get(params, "item")?;
    let name = json_get(item, "name").and_then(json_str)?;
    let uri = json_get(item, "uri").and_then(json_str)?;
    let path = uri_to_path(uri);
    let doc = server.docs.get(uri)?;
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    let mut out = String::new();
    let mut first = true;
    for edge in db
        .calls
        .iter()
        .filter(|edge| edge.module_path == path && edge.callee == name)
    {
        let caller_name = short_symbol_name(&edge.caller);
        let Some(def) = db
            .defs
            .iter()
            .find(|def| def.module_path == path && def.name == caller_name)
        else {
            continue;
        };
        let Some(item_json) = call_hierarchy_item_json(def, &doc.text) else {
            continue;
        };
        if !first {
            out.push(',');
        }
        first = false;
        let call_span = Span {
            start: edge.call_span.start,
            end: edge.call_span.end,
        };
        out.push_str(&format!(
            r#"{{"from":{},"fromRanges":[{}]}}"#,
            item_json,
            range_json(byte_span_to_range(&doc.text, call_span))
        ));
    }
    Some(response(id, &format!("[{}]", out)))
}

fn call_hierarchy_outgoing_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let item = json_get(params, "item")?;
    let name = json_get(item, "name").and_then(json_str)?;
    let uri = json_get(item, "uri").and_then(json_str)?;
    let path = uri_to_path(uri);
    let doc = server.docs.get(uri)?;
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    let mut out = String::new();
    let mut first = true;
    for edge in db.calls.iter().filter(|edge| {
        edge.module_path == path && (edge.caller == name || short_symbol_name(&edge.caller) == name)
    }) {
        let Some(def) = db
            .defs
            .iter()
            .find(|def| def.module_path == path && def.name == edge.callee)
        else {
            continue;
        };
        let Some(item_json) = call_hierarchy_item_json(def, &doc.text) else {
            continue;
        };
        if !first {
            out.push(',');
        }
        first = false;
        let call_span = Span {
            start: edge.call_span.start,
            end: edge.call_span.end,
        };
        out.push_str(&format!(
            r#"{{"to":{},"fromRanges":[{}]}}"#,
            item_json,
            range_json(byte_span_to_range(&doc.text, call_span))
        ));
    }
    Some(response(id, &format!("[{}]", out)))
}

fn prepare_type_hierarchy_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let offset = lsp_pos_to_offset(&doc.text, LspPos { line, character });

    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };
    let item = db
        .defs
        .iter()
        .find(|def| {
            def.module_path == doc.path
                && def.def_span.start <= offset
                && offset <= def.def_span.end
        })
        .and_then(|def| type_hierarchy_item_json(def, &doc.text));
    match item {
        Some(item) => Some(response(id, &format!("[{}]", item))),
        None => Some(response(id, "null")),
    }
}

fn type_hierarchy_supertypes_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let (uri, name) = type_hierarchy_item_params(params)?;
    let doc = server.docs.get(uri)?;
    let checked = server.check_with_bundle(doc);
    let bundle = checked.bundle?;
    let db = build_symbol_db(&bundle, &checked.facts);
    let mut out = Vec::new();
    for trait_name in type_hierarchy_trait_impls(&bundle, name) {
        if let Some(def) = db
            .defs
            .iter()
            .find(|def| def.name == trait_name && matches!(def.kind, SymKind::Trait | SymKind::Tag))
        {
            let src = module_source(&bundle, &def.module_path).unwrap_or(&doc.text);
            if let Some(item) = type_hierarchy_item_json(def, src) {
                out.push(item);
            }
        }
    }
    Some(response(id, &format!("[{}]", out.join(","))))
}

fn type_hierarchy_subtypes_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let (uri, name) = type_hierarchy_item_params(params)?;
    let doc = server.docs.get(uri)?;
    let checked = server.check_with_bundle(doc);
    let bundle = checked.bundle?;
    let db = build_symbol_db(&bundle, &checked.facts);
    let mut out = Vec::new();
    for subtype in type_hierarchy_subtype_names(&bundle, name) {
        if let Some(def) = db.defs.iter().find(|def| {
            def.name == subtype && matches!(def.kind, SymKind::Struct { .. } | SymKind::Enum { .. })
        }) {
            let src = module_source(&bundle, &def.module_path).unwrap_or(&doc.text);
            if let Some(item) = type_hierarchy_item_json(def, src) {
                out.push(item);
            }
        }
    }
    Some(response(id, &format!("[{}]", out.join(","))))
}

fn hover_response(server: &Server, params: Option<&JsonValue>, id: &JsonValue) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let tokens = server.lex(doc);
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    match compute_hover(&db, &tokens, &doc.text, &doc.path, offset) {
        Some(text) => {
            let result = format!(
                r#"{{"contents":{{"kind":"markdown","value":"{}"}}}}"#,
                json_escape(&text)
            );
            Some(response(id, &result))
        }
        None => Some(response(id, "null")),
    }
}

fn definition_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let tokens = server.lex(doc);
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    match compute_definition(&db, &tokens, &doc.text, &doc.path, offset) {
        Some((def_path, def_span)) => {
            let def_uri = path_to_uri(&def_path);
            let src = if def_path == doc.path {
                doc.text.clone()
            } else {
                std::fs::read_to_string(&def_path).unwrap_or_default()
            };
            let range = byte_span_to_range(&src, def_span);
            let result = format!(
                r#"{{"uri":"{}","range":{}}}"#,
                json_escape(&def_uri),
                range_json(range)
            );
            Some(response(id, &result))
        }
        None => {
            let generated = crate::Driver::query_build_plan_with_overlay(&doc.path, &doc.text)
                .ok()
                .flatten()
                .and_then(|plan| compute_generated_definition(&plan, &tokens, offset));
            let Some((relative_path, source, span)) = generated else {
                return Some(response(id, "null"));
            };
            let root = std::path::Path::new(&doc.path)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let def_path = root.join(relative_path).to_string_lossy().into_owned();
            let result = format!(
                r#"{{"uri":"{}","range":{}}}"#,
                json_escape(&path_to_uri(&def_path)),
                range_json(byte_span_to_range(&source, span))
            );
            Some(response(id, &result))
        }
    }
}

fn execute_command_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let command = json_get(params, "command").and_then(json_str)?;
    if command == "jet.budgetReports" {
        let args = match json_get(params, "arguments") {
            Some(JsonValue::Array(args)) => args,
            _ => return Some(error_response(id, -32602, "jet.budgetReports expects arguments [uri]")),
        };
        let Some(uri) = args.first().and_then(|value| match value { JsonValue::String(value) => Some(value.as_str()), _ => None }) else {
            return Some(error_response(id, -32602, "jet.budgetReports expects arguments [uri]"));
        };
        let Some(doc) = server.docs.get(uri) else { return Some(error_response(id, -32602, "document not open in LSP session")) };
        let root = workspace_root_for_path(server, &doc.path).unwrap_or_else(|| normalize_path_buf(std::path::Path::new(&doc.path).parent().unwrap_or(std::path::Path::new("."))));
        let sources = workspace_sources(server, Some(&doc.path)).into_iter().map(|(path, source)| {
            let path = std::path::Path::new(&path).strip_prefix(&root).unwrap_or(std::path::Path::new(&path)).to_string_lossy().replace('\\', "/");
            (path, crate::SHA256::sha256_hex(source.as_bytes()))
        }).collect::<Vec<_>>();
        let projection = crate::BudgetView::read_compatible(std::path::Path::new(&root), &sources);
        return Some(response(id, &projection.to_json()));
    }
    if command != "jet.impact" {
        return Some(error_response(id, -32601, "unknown executeCommand"));
    }
    let args = match json_get(params, "arguments") {
        Some(JsonValue::Array(arr)) => arr,
        _ => {
            return Some(error_response(
                id,
                -32602,
                "jet.impact expects arguments [uri, symbol, depth?]",
            ));
        }
    };
    let uri = args.first().and_then(|v| match v {
        JsonValue::String(s) => Some(s.as_str()),
        _ => None,
    });
    let symbol = args.get(1).and_then(|v| match v {
        JsonValue::String(s) => Some(s.as_str()),
        _ => None,
    });
    let depth = args
        .get(2)
        .and_then(json_int)
        .map(|n| n as usize)
        .unwrap_or(3)
        .max(1);

    let (uri, symbol) = match (uri, symbol) {
        (Some(u), Some(s)) => (u, s),
        _ => {
            return Some(error_response(
                id,
                -32602,
                "jet.impact expects arguments [uri, symbol, depth?]",
            ));
        }
    };

    let doc = match server.docs.get(uri) {
        Some(d) => d,
        None => {
            return Some(error_response(
                id,
                -32602,
                "document not open in LSP session",
            ));
        }
    };

    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => {
            return Some(error_response(id, -32603, "document did not check cleanly"));
        }
    };

    let report = jet_impact::ImpactReport::analyze(&db.index, symbol, depth);
    Some(response(id, &report.to_json()))
}

fn references_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let ctx = json_get(params, "context");
    let include_decl = ctx
        .and_then(|c| json_get(c, "includeDeclaration"))
        .and_then(|v| {
            if let JsonValue::Bool(b) = v {
                Some(*b)
            } else {
                None
            }
        })
        .unwrap_or(false);

    let tokens = server.lex(doc);
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    let refs = compute_references(&db, &tokens, &doc.path, offset, include_decl);
    let mut items = String::new();
    for (i, (ref_path, span)) in refs.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        let ref_uri = path_to_uri(ref_path);
        let src = if ref_path == &doc.path {
            doc.text.clone()
        } else {
            std::fs::read_to_string(ref_path).unwrap_or_default()
        };
        let range = byte_span_to_range(&src, *span);
        items.push_str(&format!(
            r#"{{"uri":"{}","range":{}}}"#,
            json_escape(&ref_uri),
            range_json(range)
        ));
    }
    Some(response(id, &format!("[{}]", items)))
}

fn prepare_rename_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let offset = lsp_pos_to_offset(&doc.text, LspPos { line, character });
    let tokens = server.lex(doc);
    let tok = match token_at(&tokens, offset) {
        Some(tok) => tok,
        None => return Some(response(id, "null")),
    };
    let text = token_text(&doc.text, tok);
    match &tok.kind {
        TokKind::Ident(name) => {
            let range = range_json(byte_span_to_range(&doc.text, tok.span));
            Some(response(
                id,
                &format!(
                    r#"{{"range":{},"placeholder":"{}"}}"#,
                    range,
                    json_escape(name)
                ),
            ))
        }
        _ if is_keyword_token(tok, text) => Some(error_response(
            id,
            -32600,
            &format!("`{}` is Jet syntax, not a name you can rename", text),
        )),
        _ => Some(response(id, "null")),
    }
}

fn rename_response(server: &Server, params: Option<&JsonValue>, id: &JsonValue) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);
    let new_name = json_get(params, "newName").and_then(json_str)?;

    let tokens = server.lex(doc);
    let checked = server.check_with_bundle(doc);
    let db = match checked.bundle {
        Some(b) => build_symbol_db(&b, &checked.facts),
        None => SymbolDB::new(),
    };

    match compute_rename(&db, &tokens, &doc.path, offset, new_name) {
        Ok(spans) => {
            // Group edits by file
            let mut by_file: HashMap<String, Vec<Span>> = HashMap::new();
            for (path, span) in spans {
                by_file.entry(path).or_default().push(span);
            }
            let mut changes = String::new();
            let mut first = true;
            for (path, file_spans) in &by_file {
                if !first {
                    changes.push(',');
                }
                first = false;
                let file_uri = path_to_uri(path);
                let src = if path == &doc.path {
                    doc.text.clone()
                } else {
                    std::fs::read_to_string(path).unwrap_or_default()
                };
                let mut edits = String::new();
                for (j, &span) in file_spans.iter().enumerate() {
                    if j > 0 {
                        edits.push(',');
                    }
                    let range = byte_span_to_range(&src, span);
                    edits.push_str(&format!(
                        r#"{{"range":{},"newText":"{}"}}"#,
                        range_json(range),
                        json_escape(new_name)
                    ));
                }
                changes.push_str(&format!(r#""{}": [{}]"#, json_escape(&file_uri), edits));
            }
            Some(response(id, &format!(r#"{{"changes":{{{}}}}}"#, changes)))
        }
        Err(msg) => Some(error_response(id, -32600, &msg)),
    }
}

fn semantic_tokens_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;

    let tokens = server.lex(doc);
    let data = encode_semantic_tokens(&tokens, &doc.text);
    Some(response(id, &semantic_tokens_json(&doc.text, &data)))
}

fn semantic_tokens_range_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let range = match json_get(params, "range").and_then(range_from_json) {
        Some(range) => range,
        None => return Some(response(id, r#"{"data":[]}"#)),
    };
    let start = lsp_pos_to_offset(&doc.text, range.start);
    let end = lsp_pos_to_offset(&doc.text, range.end).max(start);
    let tokens = server.lex(doc);
    let data = encode_semantic_tokens_in_span(&tokens, &doc.text, Span { start, end });
    Some(response(id, &semantic_tokens_json(&doc.text, &data)))
}

fn semantic_tokens_delta_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let tokens = server.lex(doc);
    let data = encode_semantic_tokens(&tokens, &doc.text);
    // LSP permits a delta request to return a full SemanticTokens result.
    Some(response(id, &semantic_tokens_json(&doc.text, &data)))
}

fn semantic_tokens_json(src: &str, data: &[u32]) -> String {
    let data_str: Vec<String> = data.iter().map(|n| n.to_string()).collect();
    format!(
        r#"{{"resultId":"{}","data":[{}]}}"#,
        semantic_tokens_result_id(src, data),
        data_str.join(",")
    )
}

fn semantic_tokens_result_id(src: &str, data: &[u32]) -> String {
    format!("{}:{}", src.len(), data.len())
}

fn inlay_hint_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;

    let checked = server.check_with_bundle(doc);

    // Build type-annotation hints from the symbol DB.
    let hints: Vec<InlayHint> = match checked.bundle {
        Some(b) => {
            let db = build_symbol_db(&b, &checked.facts);
            db.inlay_hints_for(&doc.path).into_iter().cloned().collect()
        }
        None => Vec::new(),
    };

    let hint_refs: Vec<&InlayHint> = hints.iter().collect();
    let json = format_inlay_hints(&hint_refs, &doc.text);
    Some(response(id, &json))
}

struct ActiveCall {
    name: String,
    active_param: usize,
}

fn active_call(src: &str, offset: usize) -> Option<ActiveCall> {
    let bytes = src.as_bytes();
    let mut i = offset.min(bytes.len());
    let mut nested = 0usize;
    let mut active_param = 0usize;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' | b']' | b'}' => nested += 1,
            b'(' if nested == 0 => {
                let name = callee_name_before_paren(bytes, i)?;
                return Some(ActiveCall { name, active_param });
            }
            b'(' | b'[' | b'{' => nested = nested.saturating_sub(1),
            b',' if nested == 0 => active_param += 1,
            _ => {}
        }
    }
    None
}

fn callee_name_before_paren(bytes: &[u8], paren: usize) -> Option<String> {
    let mut end = paren;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    if start == end {
        return None;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()
        .map(|s| s.to_string())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn token_at(tokens: &[Token], offset: usize) -> Option<&Token> {
    tokens
        .iter()
        .find(|tok| tok.span.start <= offset && offset <= tok.span.end)
}

fn token_text<'a>(src: &'a str, tok: &Token) -> &'a str {
    src.get(tok.span.start..tok.span.end.min(src.len()))
        .unwrap_or("")
}

fn is_keyword_token(tok: &Token, text: &str) -> bool {
    if crate::Syntax::JET_KEYWORD_LIST.contains(&text) {
        return true;
    }
    matches!(
        tok.kind,
        TokKind::KwWhile
            | TokKind::KwFor
            | TokKind::KwSwitch
            | TokKind::KwMutate
            | TokKind::KwMove
    )
}

fn document_symbol_kind(kind: &SymKind) -> Option<u8> {
    match kind {
        SymKind::Function { .. } => Some(12),
        SymKind::Struct { .. } => Some(23),
        SymKind::Enum { .. } => Some(10),
        SymKind::Trait | SymKind::Tag => Some(11),
        SymKind::Const => Some(14),
        _ => None,
    }
}

fn call_hierarchy_item_json(def: &jet_semindex::SymDef, src: &str) -> Option<String> {
    if !matches!(def.kind, SymKind::Function { .. }) {
        return None;
    }
    let uri = path_to_uri(&def.module_path);
    let range = range_json(byte_span_to_range(src, def.def_span));
    Some(format!(
        r#"{{"name":"{}","kind":12,"uri":"{}","range":{},"selectionRange":{}}}"#,
        json_escape(&def.name),
        json_escape(&uri),
        range,
        range
    ))
}

fn type_hierarchy_item_json(def: &jet_semindex::SymDef, src: &str) -> Option<String> {
    let kind = match def.kind {
        SymKind::Struct { .. } => 23,
        SymKind::Enum { .. } => 10,
        SymKind::Trait | SymKind::Tag => 11,
        _ => return None,
    };
    let uri = path_to_uri(&def.module_path);
    let range = range_json(byte_span_to_range(src, def.def_span));
    Some(format!(
        r#"{{"name":"{}","kind":{},"uri":"{}","range":{},"selectionRange":{}}}"#,
        json_escape(&def.name),
        kind,
        json_escape(&uri),
        range,
        range
    ))
}

fn type_hierarchy_item_params(params: Option<&JsonValue>) -> Option<(&str, &str)> {
    let item = json_get(params?, "item")?;
    let uri = json_get(item, "uri").and_then(json_str)?;
    let name = json_get(item, "name").and_then(json_str)?;
    Some((uri, name))
}

fn merge_workspace_defs(server: &Server, current: &Document, db: &mut SymbolDB) {
    for (path, text) in workspace_sources(server, Some(&current.path)) {
        if path == current.path {
            continue;
        }
        let (_diags, bundle, facts) = check_document_with_bundle(&path, &text);
        if let Some(bundle) = bundle {
            let mut other = build_symbol_db(&bundle, &facts);
            db.symbols.extend(other.symbols.symbols().iter().cloned());
            db.defs.append(&mut other.defs);
            db.refs.append(&mut other.refs);
            db.calls.append(&mut other.calls);
            db.hover.append(&mut other.hover);
            db.inlay.append(&mut other.inlay);
        }
    }
}

fn workspace_sources(server: &Server, root_hint: Option<&str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for doc in server.docs.values() {
        if seen.insert(doc.path.clone()) {
            out.push((doc.path.clone(), doc.text.clone()));
        }
    }
    let Some(path) = root_hint else {
        return out;
    };
    let root = workspace_root_for_path(server, path).unwrap_or_else(|| {
        normalize_path_buf(
            std::path::Path::new(path)
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
    });
    let mut files = Vec::new();
    collect_jet_files(std::path::Path::new(&root), &mut files);
    files.sort();
    for path in files.into_iter().take(256) {
        let display = path.to_string_lossy().to_string();
        if seen.contains(&display) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            seen.insert(display.clone());
            out.push((display, text));
        }
    }
    out
}

fn collect_jet_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "target" | "node_modules") {
            continue;
        }
        if path.is_dir() {
            collect_jet_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("jet") {
            out.push(path);
        }
    }
}

fn module_source<'a>(bundle: &'a ProgramBundle, path: &str) -> Option<&'a str> {
    bundle
        .modules
        .iter()
        .find(|module| module.display == path)
        .map(|module| module.source.as_str())
}

fn type_hierarchy_trait_impls(bundle: &ProgramBundle, type_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        for item in &module.items {
            match item {
                crate::AST::Item::Struct(s) if s.name == type_name => {
                    out.extend(s.trait_impls.iter().map(|tb| tb.trait_name.clone()));
                }
                crate::AST::Item::Enum(e) if e.name == type_name => {
                    out.extend(e.trait_impls.iter().map(|tb| tb.trait_name.clone()));
                }
                _ => {}
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn type_hierarchy_subtype_names(bundle: &ProgramBundle, trait_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        for item in &module.items {
            match item {
                crate::AST::Item::Struct(s)
                    if s.trait_impls.iter().any(|tb| tb.trait_name == trait_name) =>
                {
                    out.push(s.name.clone());
                }
                crate::AST::Item::Enum(e)
                    if e.trait_impls.iter().any(|tb| tb.trait_name == trait_name) =>
                {
                    out.push(e.name.clone());
                }
                _ => {}
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn short_symbol_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

#[derive(Clone, Copy)]
struct FoldingRange {
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
    kind: Option<&'static str>,
}

impl FoldingRange {
    fn to_json(self) -> String {
        let kind = self
            .kind
            .map(|kind| format!(r#","kind":"{}""#, kind))
            .unwrap_or_default();
        format!(
            r#"{{"startLine":{},"startCharacter":{},"endLine":{},"endCharacter":{}{}}}"#,
            self.start_line, self.start_character, self.end_line, self.end_character, kind
        )
    }
}

#[derive(Clone, Copy)]
struct BlockSpan {
    start: usize,
    end: usize,
}

fn compute_folding_ranges(src: &str) -> Vec<FoldingRange> {
    let mut folds = brace_folding_ranges(src);
    folds.extend(line_group_folds(src, is_comment_line, "comment"));
    folds.extend(line_group_folds(src, is_import_line, "imports"));
    folds.sort_by_key(|fold| (fold.start_line, fold.start_character, fold.end_line));
    folds
}

fn brace_folding_ranges(src: &str) -> Vec<FoldingRange> {
    let mut stack: Vec<LspPos> = Vec::new();
    let mut folds = Vec::new();
    for (offset, ch) in src.char_indices() {
        match ch {
            '{' => stack.push(byte_offset_to_lsp(src, offset)),
            '}' => {
                if let Some(start) = stack.pop() {
                    let end = byte_offset_to_lsp(src, offset);
                    if end.line > start.line {
                        folds.push(FoldingRange {
                            start_line: start.line,
                            start_character: start.character,
                            end_line: end.line,
                            end_character: end.character,
                            kind: Some("region"),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    folds
}

fn line_group_folds(
    src: &str,
    predicate: fn(&str) -> bool,
    kind: &'static str,
) -> Vec<FoldingRange> {
    let lines = src.lines().collect::<Vec<_>>();
    let mut folds = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if predicate(line) {
            start.get_or_insert(idx);
            continue;
        }
        if let Some(first) = start.take() {
            push_line_group_fold(&lines, first, idx.saturating_sub(1), kind, &mut folds);
        }
    }
    if let Some(first) = start.take() {
        push_line_group_fold(
            &lines,
            first,
            lines.len().saturating_sub(1),
            kind,
            &mut folds,
        );
    }
    folds
}

fn push_line_group_fold(
    lines: &[&str],
    start: usize,
    end: usize,
    kind: &'static str,
    folds: &mut Vec<FoldingRange>,
) {
    if end <= start {
        return;
    }
    folds.push(FoldingRange {
        start_line: start as u32,
        start_character: first_non_ws(lines[start]) as u32,
        end_line: end as u32,
        end_character: lines[end].chars().count() as u32,
        kind: Some(kind),
    });
}

fn is_comment_line(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

fn is_import_line(line: &str) -> bool {
    line.trim_start().starts_with("import ")
}

fn first_non_ws(line: &str) -> usize {
    line.chars().position(|ch| !ch.is_whitespace()).unwrap_or(0)
}

fn ident_at<'a>(tokens: &'a [Token], offset: usize) -> Option<&'a str> {
    tokens.iter().find_map(|tok| {
        if tok.span.start <= offset && offset <= tok.span.end {
            if let TokKind::Ident(name) = &tok.kind {
                return Some(name.as_str());
            }
        }
        None
    })
}

fn brace_block_spans(src: &str) -> Vec<BlockSpan> {
    let mut stack: Vec<usize> = Vec::new();
    let mut blocks = Vec::new();
    for (offset, ch) in src.char_indices() {
        match ch {
            '{' => stack.push(offset),
            '}' => {
                if let Some(start) = stack.pop() {
                    blocks.push(BlockSpan {
                        start,
                        end: offset + ch.len_utf8(),
                    });
                }
            }
            _ => {}
        }
    }
    blocks
}

fn selection_ranges_for(
    src: &str,
    tokens: &[Token],
    blocks: &[BlockSpan],
    offset: usize,
) -> Vec<LspRange> {
    let mut ranges = Vec::new();
    if let Some(span) = token_span_at(tokens, offset) {
        ranges.push(byte_span_to_range(src, span));
    }
    if let Some(span) = line_span_at(src, offset) {
        ranges.push(byte_span_to_range(src, span));
    }
    if let Some(block) = blocks
        .iter()
        .filter(|block| block.start <= offset && offset <= block.end)
        .min_by_key(|block| block.end.saturating_sub(block.start))
    {
        ranges.push(byte_span_to_range(
            src,
            Span {
                start: block.start,
                end: block.end,
            },
        ));
    }
    ranges.push(full_document_range(src));
    dedup_ranges(ranges)
}

fn token_span_at(tokens: &[Token], offset: usize) -> Option<Span> {
    tokens
        .iter()
        .find(|tok| {
            tok.span.start <= offset && offset <= tok.span.end && tok.span.start < tok.span.end
        })
        .map(|tok| tok.span)
}

fn line_span_at(src: &str, offset: usize) -> Option<Span> {
    if src.is_empty() {
        return None;
    }
    let offset = offset.min(src.len());
    let start = src[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(src.len());
    Some(Span { start, end })
}

fn dedup_ranges(ranges: Vec<LspRange>) -> Vec<LspRange> {
    let mut out = Vec::new();
    for range in ranges {
        if !out.iter().any(|existing| same_range(*existing, range)) {
            out.push(range);
        }
    }
    out
}

fn same_range(a: LspRange, b: LspRange) -> bool {
    a.start == b.start && a.end == b.end
}

fn selection_range_json(ranges: &[LspRange]) -> String {
    let mut iter = ranges.iter().rev();
    let Some(last) = iter.next() else {
        return r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}"#
            .to_string();
    };
    let mut json = format!(r#"{{"range":{}}}"#, range_json(*last));
    for range in iter {
        json = format!(r#"{{"range":{},"parent":{}}}"#, range_json(*range), json);
    }
    json
}

fn document_links_for(path: &str, workspace_root: Option<&str>, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let fallback_base = std::path::Path::new(path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let base = workspace_root
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(normalize_path_buf(fallback_base)));
    for (line_idx, line) in src.lines().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("use ") {
            continue;
        }
        let indent = line.len().saturating_sub(trimmed.len());
        let path_start = indent + "use ".len();
        let after_use = &line[path_start..];
        let import_len = after_use
            .find(|c: char| c.is_whitespace() || c == '{')
            .unwrap_or(after_use.len());
        if import_len == 0 {
            continue;
        }
        let import = &after_use[..import_len];
        let Some(target) = resolve_use_target(&base, import) else {
            continue;
        };
        let range = LspRange {
            start: LspPos {
                line: line_idx as u32,
                character: path_start as u32,
            },
            end: LspPos {
                line: line_idx as u32,
                character: (path_start + import_len) as u32,
            },
        };
        out.push(format!(
            r#"{{"range":{},"target":"{}"}}"#,
            range_json(range),
            json_escape(&target)
        ));
    }
    out
}

fn resolve_use_target(base: &std::path::Path, import: &str) -> Option<String> {
    if import.starts_with("core.") {
        let doc =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/reference/core-library.md");
        return doc.exists().then(|| path_to_uri(&doc.to_string_lossy()));
    }
    let rel = import.replace('.', "/") + ".jet";
    let path = base.join(rel);
    path.exists().then(|| path_to_uri(&path.to_string_lossy()))
}

fn workspace_root_for_path(server: &Server, path: &str) -> Option<String> {
    let path = normalize_path(path);
    let path_ref = std::path::Path::new(&path);
    server
        .workspace_roots
        .iter()
        .filter(|root| path_ref.starts_with(root.as_str()))
        .max_by_key(|root| root.len())
        .cloned()
        .or_else(|| project_root_marker(&path))
}

fn project_root_marker(path: &str) -> Option<String> {
    let mut dir = std::path::Path::new(path).parent()?;
    loop {
        if dir.join("pkg.jet").exists()
            || dir.join("Jet.toml").exists()
            || dir.join("jet.toml").exists()
            || dir.join(".git").exists()
        {
            return Some(normalize_path_buf(dir));
        }
        dir = dir.parent()?;
    }
}

fn code_lenses_for(uri: &str, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len().saturating_sub(trimmed.len());
        if trimmed.starts_with("fn run") {
            out.push(code_lens_json(
                idx,
                indent,
                line.len(),
                "Run file",
                "jet.runFile",
                &[uri.to_string()],
            ));
        } else if trimmed.starts_with("@Test") {
            out.push(code_lens_json(
                idx,
                indent,
                line.len(),
                "Run test",
                "jet.testFile",
                &[uri.to_string(), idx.to_string()],
            ));
        }
    }
    out
}

fn code_lens_json(
    line: usize,
    start: usize,
    end: usize,
    title: &str,
    command: &str,
    args: &[String],
) -> String {
    let range = LspRange {
        start: LspPos {
            line: line as u32,
            character: start as u32,
        },
        end: LspPos {
            line: line as u32,
            character: end as u32,
        },
    };
    let arguments = args
        .iter()
        .map(|arg| format!(r#""{}""#, json_escape(arg)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"range":{},"command":{{"title":"{}","command":"{}","arguments":[{}]}}}}"#,
        range_json(range),
        json_escape(title),
        json_escape(command),
        arguments
    )
}

// ── URI / path utilities ──────────────────────────────────────────────────────

fn uri_to_path(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("file://") {
        if cfg!(windows) {
            rest.trim_start_matches('/').replace('/', "\\")
        } else {
            rest.to_string()
        }
    } else {
        uri.to_string()
    }
}

fn path_to_uri(path: &str) -> String {
    if path.starts_with('/') || (cfg!(windows) && path.contains(':')) {
        format!("file://{}", path)
    } else {
        format!("file://{}", path)
    }
}

fn normalize_path(path: &str) -> String {
    normalize_path_buf(std::path::Path::new(path))
}

fn normalize_path_buf(path: &std::path::Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn load_discovery_index(path: &str) -> Option<jetpack::Discovery::Index> {
    let mut dir = std::path::Path::new(path).parent()?;
    loop {
        if let Ok(Some(index)) = jetpack::Discovery::load(dir) {
            return Some(index);
        }
        dir = match dir.parent() {
            Some(parent) => parent,
            None => break,
        };
    }
    None
}
