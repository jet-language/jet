//! JSON-RPC transport over stdio + request/notification dispatch + handlers.

use crate::Diagnostics::{Diagnostic, Severity, Span};
use crate::Lexer::{TokKind, Token};
use crate::AST::ProgramBundle;
use jet_driver::QueryService::CompilerQueries;
#[cfg(test)]
use jet_queries::{FileKey, QueryKey};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use super::Check::{fixes_from_diagnostics, Fix};
use super::Completion::compute_completions;
use super::Features::{
    compute_definition, compute_generated_definition, compute_hover, compute_references,
    compute_refactor_actions, compute_rename, encode_semantic_tokens,
    encode_semantic_tokens_in_span, format_inlay_hints, RefactorAction,
};
use super::Position::{
    apply_lsp_edit, byte_offset_to_lsp, byte_span_to_range, full_document_range, lsp_pos_to_offset,
    range_json, LspPos, LspRange,
};
use super::SymbolDB::{build_symbol_db, InlayHint, SymKind, SymbolDB};
use jet_foundation::JSON::{
    json_escape, json_get, json_str, json_u32, parse_json, read_protocol_content_length, JsonValue,
    MAX_PROTOCOL_MESSAGE_BYTES,
};

// ── Document state ────────────────────────────────────────────────────────────

struct Document {
    path: String,
    text: String,
    version: i32,
}

#[derive(Clone)]
struct CheckedBundle {
    diags: Vec<Diagnostic>,
    bundle: Option<Arc<ProgramBundle>>,
    facts: jet_semindex::SemIndexEffectFacts,
}

impl Document {
    fn new(path: String, text: String, version: i32) -> Self {
        Document {
            path,
            text,
            version,
        }
    }

    fn replace_text(&mut self, text: String) {
        self.text = text;
    }

    fn apply_range_edit(
        &mut self,
        range: LspRange,
        range_length: Option<u32>,
        text: &str,
    ) -> bool {
        let Some(text) = apply_lsp_edit(&self.text, range, range_length, text) else {
            return false;
        };
        self.text = text;
        true
    }
}

struct Server {
    docs: HashMap<String, Document>,
    workspace_roots: Vec<String>,
    workspace_folders: bool,
    /// URIs of documents that changed since last diagnostic publish (D-LSP3).
    dirty: std::collections::HashSet<String>,
    /// D-LSP1=C: the canonical driver query service shared with `jet check`.
    queries: std::cell::RefCell<CompilerQueries>,
    shutdown: bool,
}

impl Server {
    fn new() -> Self {
        Server {
            docs: HashMap::new(),
            workspace_roots: Vec::new(),
            workspace_folders: false,
            dirty: std::collections::HashSet::new(),
            queries: std::cell::RefCell::new(CompilerQueries::new()),
            shutdown: false,
        }
    }

    /// D-LSP1 stage 2: diagnostics run through the query engine instead of a
    /// private LSP-only cache.
    fn check(&self, doc: &Document) -> Vec<Diagnostic> {
        self.check_with_bundle(doc).diags
    }

    fn check_with_bundle(&self, doc: &Document) -> CheckedBundle {
        let mut queries = self.queries.borrow_mut();
        for open in self.docs.values() {
            queries.set_document(&open.path, &open.text);
        }
        let checked = queries.check_text(&doc.path, &doc.text, true);
        CheckedBundle {
            diags: checked.diagnostics.as_ref().clone(),
            bundle: checked.bundle,
            facts: checked.effect_facts.as_ref().clone(),
        }
    }

    fn lex(&self, doc: &Document) -> Arc<Vec<Token>> {
        self.queries
            .borrow_mut()
            .lex_text(&doc.path, &doc.text)
    }

}

// ── JSON-RPC main loop ────────────────────────────────────────────────────────

pub fn run_stdio() -> io::Result<()> {
    let (messages, incoming) = std::sync::mpsc::channel();
    let cancelled = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let reader_cancelled = Arc::clone(&cancelled);
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        loop {
            let body = match read_message(&mut stdin) {
                Ok(Some(body)) => body,
                Ok(None) => break,
                Err(error) => {
                    let _ = messages.send(Err(error));
                    break;
                }
            };
            if let Ok(message) = parse_rpc_message(&body) {
                if json_get(&message, "method").and_then(json_str) == Some("$/cancelRequest") {
                    if let Some(key) = cancellation_key(json_get(&message, "params")) {
                        lock_cancelled(&reader_cancelled).insert(key);
                    }
                }
            }
            if messages.send(Ok(body)).is_err() {
                break;
            }
        }
    });
    let mut stdout = io::stdout();
    let mut server = Server::new();

    while let Ok(body) = incoming.recv() {
        let body = body?;
        let msg = match parse_rpc_message(&body) {
            Ok(v) => v,
            Err((code, message)) => {
                write_message(
                    &mut stdout,
                    &error_response(&JsonValue::Null, code, message),
                )?;
                continue;
            }
        };
        let method = json_get(&msg, "method").and_then(json_str);
        let id = json_get(&msg, "id").cloned();
        let params = json_get(&msg, "params");

        if let Some(method) = method {
            if id.is_some() {
                // D-LSP3: flush any buffered dirty-document diagnostics before serving requests.
                let _ = flush_dirty(&mut server, &mut stdout);
                let id = id.as_ref().unwrap();
                let key = serialize_id(id);
                let resp = cancellable_response(&cancelled, &key, id, || {
                    catch_handler(std::panic::AssertUnwindSafe(|| {
                        handle_request(&mut server, method, params, id)
                    }))
                });
                if let Some(resp) = resp {
                    write_message(&mut stdout, &resp)?;
                }
            } else {
                let cancel_key = (method == "$/cancelRequest")
                    .then(|| cancellation_key(params))
                    .flatten();
                let result = catch_notification(|| {
                    handle_notification(&mut server, method, params, &mut stdout)
                });
                if let Some(key) = cancel_key {
                    // This queued notification can only be observed after the
                    // state-owning loop finished the matching request. Drop a
                    // late ID so a future reused JSON-RPC ID is not cancelled.
                    lock_cancelled(&cancelled).remove(&key);
                }
                result?;
            }
        }

        if server.shutdown {
            break;
        }
    }
    Ok(())
}

fn lock_cancelled(
    cancelled: &Mutex<std::collections::HashSet<String>>,
) -> std::sync::MutexGuard<'_, std::collections::HashSet<String>> {
    cancelled
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn cancellation_key(params: Option<&JsonValue>) -> Option<String> {
    let id = params.and_then(|params| json_get(params, "id"))?;
    matches!(id, JsonValue::Number(_) | JsonValue::String(_)).then(|| serialize_id(id))
}

fn cancellable_response<F>(
    cancelled: &Mutex<std::collections::HashSet<String>>,
    key: &str,
    id: &JsonValue,
    run: F,
) -> Option<String>
where
    F: FnOnce() -> Option<String>,
{
    if lock_cancelled(cancelled).remove(key) {
        return Some(error_response(id, -32800, "Request cancelled"));
    }
    let response = run();
    if lock_cancelled(cancelled).remove(key) {
        Some(error_response(id, -32800, "Request cancelled"))
    } else {
        response
    }
}

fn parse_rpc_message(body: &str) -> Result<JsonValue, (i64, &'static str)> {
    let message = parse_json(body).map_err(|()| (-32700, "Parse error"))?;
    let JsonValue::Object(_) = &message else {
        return Err((-32600, "Invalid Request"));
    };
    if json_get(&message, "jsonrpc").and_then(json_str) != Some("2.0")
        || json_get(&message, "method").and_then(json_str).is_none()
        || !matches!(
            json_get(&message, "id"),
            None | Some(JsonValue::Null | JsonValue::Number(_) | JsonValue::String(_))
        )
        || !matches!(
            json_get(&message, "params"),
            None | Some(JsonValue::Array(_) | JsonValue::Object(_))
        )
    {
        return Err((-32600, "Invalid Request"));
    }
    Ok(message)
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
    let len = match read_protocol_content_length(reader)? {
        Some(l) => l,
        None => return Ok(None),
    };
    if len > MAX_PROTOCOL_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "protocol message exceeds the 1048576-byte limit",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "protocol message is not UTF-8"))
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
        "textDocument/rangeFormatting" => range_format_response(server, params, id),
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
        server.workspace_folders = json_get(params, "workspaceFolders").is_some();
        if let Some(folders) = json_get(params, "workspaceFolders") {
            if let JsonValue::Array(folders) = folders {
                for folder in folders {
                    if let Some(uri) = json_get(folder, "uri").and_then(json_str) {
                        push_workspace_root(&mut roots, uri_to_path(uri));
                    }
                }
            }
        } else if let Some(root_uri) = json_get(params, "rootUri") {
            if let Some(uri) = json_str(root_uri) {
                push_workspace_root(&mut roots, uri_to_path(uri));
            }
        } else if let Some(path) = json_get(params, "rootPath").and_then(json_str) {
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

fn update_workspace_roots(server: &mut Server, params: Option<&JsonValue>) {
    let Some(event) = params.and_then(|params| json_get(params, "event")) else {
        return;
    };
    server.workspace_folders = true;
    if let Some(JsonValue::Array(removed)) = json_get(event, "removed") {
        for folder in removed {
            let Some(uri) = json_get(folder, "uri").and_then(json_str) else {
                continue;
            };
            let path = normalize_path(&uri_to_path(uri));
            server.workspace_roots.retain(|root| root != &path);
        }
    }
    if let Some(JsonValue::Array(added)) = json_get(event, "added") {
        for folder in added {
            if let Some(uri) = json_get(folder, "uri").and_then(json_str) {
                push_workspace_root(&mut server.workspace_roots, uri_to_path(uri));
            }
        }
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
        "$/cancelRequest" => {
            // The reader thread records this before the queued notification
            // reaches the state-owning loop.
            Ok(())
        }
        "exit" => {
            server.shutdown = true;
            Ok(())
        }
        "textDocument/didOpen" => publish_after_open(server, params, stdout),
        "textDocument/didChange" => publish_after_change(server, params, stdout),
        "workspace/didChangeWorkspaceFolders" => {
            update_workspace_roots(server, params);
            Ok(())
        }
        "textDocument/didClose" => {
            if let Some(uri) = params
                .and_then(|p| json_get(p, "textDocument"))
                .and_then(|td| json_get(td, "uri"))
                .and_then(json_str)
            {
                if let Some(doc) = server.docs.remove(uri) {
                    server.queries.borrow_mut().remove_document(&doc.path);
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
    "workspace": {
      "workspaceFolders": {
        "supported": true,
        "changeNotifications": true
      }
    },
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
    let Some(version) = json_get(td, "version").and_then(|value| match value {
        JsonValue::Number(version) => i32::try_from(*version).ok(),
        _ => None,
    }) else {
        return Ok(());
    };

    if is_open {
        let Some(text) = json_get(td, "text").and_then(json_str) else {
            return Ok(());
        };
        server.docs.insert(
            uri.clone(),
            Document::new(uri_to_path(&uri), text.to_string(), version),
        );
    } else {
        let Some(JsonValue::Array(changes)) = json_get(params, "contentChanges") else {
            return Ok(());
        };
        let Some(current) = server.docs.get(&uri) else {
            return Ok(());
        };
        if version <= current.version {
            return Ok(());
        }
        let mut updated = Document::new(current.path.clone(), current.text.clone(), version);
        if !changes
            .iter()
            .all(|change| apply_content_change(&mut updated, change))
        {
            return Ok(());
        }
        server.docs.insert(uri.clone(), updated);
    }
    if is_open {
        // Always publish on open — client expects initial diagnostics.
        if let Some(doc) = server.docs.get(&uri) {
            let diags = server.check(doc);
            let file = workspace_relative_diagnostic_path(server, &doc.path);
            let notif = publish_diagnostics(&uri, &file, &doc.text, doc.version, &diags);
            write_message(stdout, &notif)?;
        }
        server.dirty.remove(&uri);
    } else {
        // Mark dirty; diagnostics will be flushed before the next request.
        server.dirty.insert(uri);
    }
    Ok(())
}

fn apply_content_change(doc: &mut Document, change: &JsonValue) -> bool {
    let obj = match change {
        JsonValue::Object(obj) => obj,
        _ => return false,
    };
    let text = match obj.get("text").and_then(json_str) {
        Some(text) => text,
        None => return false,
    };
    let range_length = match obj.get("rangeLength") {
        Some(value) => match lsp_uinteger(value) {
            Some(length) => Some(length),
            None => return false,
        },
        None => None,
    };
    match obj.get("range") {
        Some(value) => match range_from_json(value) {
            Some(range) => doc.apply_range_edit(range, range_length, text),
            None => false,
        },
        None if range_length.is_some() => false,
        None => {
            doc.replace_text(text.to_string());
            true
        }
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
        line: lsp_uinteger(json_get(value, "line")?)?,
        character: lsp_uinteger(json_get(value, "character")?)?,
    })
}

fn lsp_uinteger(value: &JsonValue) -> Option<u32> {
    json_u32(value).filter(|value| *value <= i32::MAX as u32)
}

/// Flush any pending dirty-document diagnostics before handling a request (D-LSP3).
fn flush_dirty(server: &mut Server, stdout: &mut impl Write) -> io::Result<()> {
    let dirty: Vec<String> = server.dirty.drain().collect();
    for uri in dirty {
        if let Some(doc) = server.docs.get(&uri) {
            let text = doc.text.clone();
            let diags = server.check(doc);
            let file = workspace_relative_diagnostic_path(server, &doc.path);
            let notif = publish_diagnostics(&uri, &file, &text, doc.version, &diags);
            write_message(stdout, &notif)?;
        }
    }
    Ok(())
}

fn publish_diagnostics(
    uri: &str,
    file: &str,
    src: &str,
    version: i32,
    diags: &[Diagnostic],
) -> String {
    let mut items = String::new();
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        items.push_str(&diagnostic_json(d, file, src));
    }
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","version":{},"diagnostics":[{}]}}}}"#,
        json_escape(uri),
        version,
        items
    )
}

fn diagnostic_json(d: &Diagnostic, file: &str, src: &str) -> String {
    let severity = match d.severity {
        Severity::Error => 1,
        Severity::Lint => 2,
    };
    let range = d
        .span
        .map(|s| byte_span_to_range(src, s))
        .unwrap_or(full_document_range(src));
    let data = d
        .structured_json(file, src)
        .map(|json| format!(r#", "data":{}"#, json))
        .unwrap_or_default();
    format!(
        r#"{{"range":{},"severity":{},"code":"{}","source":"jet","message":"{}"{}}}"#,
        range_json(range),
        severity,
        json_escape(&d.code),
        json_escape(&d.what),
        data,
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
    let requested = json_get(params, "range").and_then(range_from_json)?;
    let requested = Span::new(
        lsp_pos_to_offset(&doc.text, requested.start),
        lsp_pos_to_offset(&doc.text, requested.end),
    );
    let checked = server.check_with_bundle(doc);
    let fixes = fixes_from_diagnostics(checked.diags.clone());
    let mut db = checked
        .bundle
        .map(|bundle| build_symbol_db(&bundle, &checked.facts))
        .unwrap_or_else(SymbolDB::new);
    let import_sources = merge_workspace_defs_with_sources(server, doc, &mut db);
    let tokens = server.lex(doc);
    let workspace_root = workspace_root_for_path(server, &doc.path);
    let excluded_import_paths = server
        .docs
        .values()
        .filter(|open| {
            std::fs::read_to_string(&open.path)
                .map(|saved| saved != open.text)
                .unwrap_or(true)
        })
        .map(|open| open.path.clone())
        .collect();
    let refactors = compute_refactor_actions(
        &db,
        &tokens,
        &checked.diags,
        &doc.text,
        &doc.path,
        workspace_root.as_deref(),
        &import_sources,
        &excluded_import_paths,
        requested,
    );
    // Go through the SAME unified fix engine the CLI `jet fix` uses, so a fix
    // offered in the editor is byte-identical to a fix applied on the command
    // line.
    let mut actions = String::new();
    for (n, fix) in fixes.iter().enumerate() {
        if n > 0 {
            actions.push(',');
        }
        actions.push_str(&code_action_json(uri, doc.version, &doc.text, fix));
    }
    for (n, action) in refactors.iter().enumerate() {
        if !fixes.is_empty() || n > 0 {
            actions.push(',');
        }
        actions.push_str(&refactor_action_json(
            uri,
            doc.version,
            &doc.text,
            action,
        ));
    }
    Some(response(id, &format!("[{}]", actions)))
}

fn code_action_json(uri: &str, version: i32, src: &str, fix: &Fix) -> String {
    action_json(
        uri,
        version,
        src,
        &fix.title,
        "quickfix",
        std::slice::from_ref(&fix.edit),
    )
}

fn refactor_action_json(
    uri: &str,
    version: i32,
    src: &str,
    action: &RefactorAction,
) -> String {
    action_json(
        uri,
        version,
        src,
        &action.title,
        action.kind,
        &action.edits,
    )
}

fn action_json(
    uri: &str,
    version: i32,
    src: &str,
    title: &str,
    kind: &str,
    edits: &[crate::Diagnostics::TextEdit],
) -> String {
    let edits = edits
        .iter()
        .map(|edit| {
            format!(
                r#"{{"range":{},"newText":"{}"}}"#,
                range_json(byte_span_to_range(src, edit.span)),
                json_escape(&edit.new_text)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"title":"{}","kind":"{}","edit":{{"documentChanges":[{{"textDocument":{{"uri":"{}","version":{}}},"edits":[{}]}}]}}}}"#,
        json_escape(title),
        kind,
        json_escape(uri),
        version,
        edits,
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

fn range_format_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let requested = range_from_json(json_get(params, "range")?)?;
    let Some((range, new_text)) = format_requested_lines(&doc.text, requested) else {
        return Some(response(id, "[]"));
    };
    Some(response(
        id,
        &format!(
            r#"[{{"range":{},"newText":"{}"}}]"#,
            range_json(range),
            json_escape(&new_text)
        ),
    ))
}

fn format_requested_lines(src: &str, requested: LspRange) -> Option<(LspRange, String)> {
    apply_lsp_edit(src, requested, None, "")?;
    let formatted = crate::format_source(src).ok()?;
    let start_line = requested.start.line;
    let end_line = requested
        .end
        .line
        .saturating_add(u32::from(requested.end.character > 0));
    if end_line < start_line {
        return None;
    }
    let source_start = line_boundary(src, start_line)?;
    let source_end = line_boundary(src, end_line).unwrap_or(src.len());
    let formatted_start = line_boundary(&formatted, start_line)?;
    let formatted_end = line_boundary(&formatted, end_line).unwrap_or(formatted.len());
    let range = byte_span_to_range(
        src,
        crate::Diagnostics::Span::new(source_start, source_end),
    );
    let new_text = formatted[formatted_start..formatted_end].to_string();
    (src[source_start..source_end] != new_text).then_some((range, new_text))
}

fn line_boundary(src: &str, line: u32) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    let mut current = 0u32;
    let bytes = src.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if matches!(bytes[index], b'\r' | b'\n') {
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            current += 1;
            if current == line {
                return Some(index + 1);
            }
        }
        index += 1;
    }
    None
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
        SymKind::Function { params, ret, effects, effect_via } => {
            let parts: Vec<String> = params
                .iter()
                .map(|(name, ty)| format!("{}: {}", name, ty.name()))
                .collect();
            let mut label = format!("fn {}({})", def.name, parts.join(", "));
            if let Some((param, _)) = effect_via {
                label.push_str(" --[via ");
                label.push_str(param);
                label.push_str("]->");
            } else if let Some(effects) = effects {
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
    let mut docs = workspace_sources(server, None);
    docs.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, text) in docs {
        let db = if let Some(doc) = server.docs.values().find(|doc| doc.path == path) {
            let checked = server.check_with_bundle(doc);
            match checked.bundle {
                Some(b) => build_symbol_db(&b, &checked.facts),
                None => SymbolDB::new(),
            }
        } else {
            let checked = server.queries.borrow_mut().check_text(&path, &text, true);
            match checked.bundle {
                Some(b) => build_symbol_db(&b, &checked.effect_facts),
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
        let line = match json_get(pos, "line").and_then(json_u32) {
            Some(line) => line,
            None => continue,
        };
        let character = match json_get(pos, "character").and_then(json_u32) {
            Some(character) => character,
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
        .and_then(json_u32)
        .map(|n| n as usize)
        .unwrap_or(3)
        .clamp(1, 64);

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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
    let line = json_u32(json_get(pos, "line")?)?;
    let character = json_u32(json_get(pos, "character")?)?;
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
        SymKind::Type => Some(5),
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
    let _ = merge_workspace_defs_with_sources(server, current, db);
}

fn merge_workspace_defs_with_sources(
    server: &Server,
    current: &Document,
    db: &mut SymbolDB,
) -> HashMap<String, String> {
    let mut sources = HashMap::new();
    for (path, text) in workspace_sources(server, Some(&current.path)) {
        if path == current.path {
            continue;
        }
        sources.insert(path.clone(), text.clone());
        let checked = server.queries.borrow_mut().check_text(&path, &text, true);
        if let Some(bundle) = checked.bundle {
            let mut other = build_symbol_db(&bundle, &checked.effect_facts);
            db.symbols.extend(other.symbols.symbols().iter().cloned());
            db.defs.append(&mut other.defs);
            db.refs.append(&mut other.refs);
            db.calls.append(&mut other.calls);
            db.hover.append(&mut other.hover);
            db.inlay.append(&mut other.inlay);
        }
    }
    sources
}

fn workspace_sources(server: &Server, root_hint: Option<&str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut roots = server.workspace_roots.clone();
    if roots.is_empty() {
        if let Some(path) = root_hint {
            roots.push(workspace_root_for_path(server, path).unwrap_or_else(|| {
                normalize_path_buf(
                    std::path::Path::new(path)
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new(".")),
                )
            }));
        }
    }
    for doc in server.docs.values() {
        let in_workspace = roots
            .iter()
            .any(|root| std::path::Path::new(&doc.path).starts_with(root));
        if (root_hint.is_some()
            || (!server.workspace_folders && roots.is_empty())
            || in_workspace)
            && seen.insert(doc.path.clone())
        {
            out.push((doc.path.clone(), doc.text.clone()));
        }
    }
    let overlays = server
        .docs
        .values()
        .map(|doc| (std::path::PathBuf::from(&doc.path), doc.text.clone()))
        .collect::<Vec<_>>();
    for root in roots {
        let root = std::path::Path::new(&root);
        let project_parts = crate::ProjectParts::scan_with_overlays(root, &overlays);
        let mut files = Vec::new();
        collect_jet_files(root, &mut files);
        files.sort();
        for path in files
            .into_iter()
            .filter(|path| project_parts.should_index(path))
            .take(256)
        {
            let display = path.to_string_lossy().to_string();
            if seen.contains(&display) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                seen.insert(display.clone());
                out.push((display, text));
            }
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
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_jet_files(&path, out);
        } else if file_type.is_file()
            && path.extension().and_then(|s| s.to_str()) == Some("jet")
        {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod project_part_tests {
    use super::*;

    fn code_actions_for(
        server: &Server,
        uri: &str,
        src: &str,
        start: usize,
        end: usize,
    ) -> String {
        let start = byte_offset_to_lsp(src, start);
        let end = byte_offset_to_lsp(src, end);
        let params = parse_json(&format!(
            r#"{{"textDocument":{{"uri":"{}"}},"range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"context":{{"diagnostics":[]}}}}"#,
            json_escape(uri),
            start.line,
            start.character,
            end.line,
            end.character,
        ))
        .unwrap();
        code_action_response(server, Some(&params), &JsonValue::Number(1)).unwrap()
    }

    #[test]
    fn code_actions_extract_binding_function_and_inline_with_versioned_edits() {
        let src =
            "fn run(left: Bool, right: Bool) {\n    total :: left && right\n    print(total)\n}\n";
        let root = std::env::temp_dir().join(format!(
            "jet-lsp-refactor-actions-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        std::fs::write(&main, src).unwrap();
        let path = main.to_string_lossy().into_owned();
        let uri = path_to_uri(&path);
        let mut server = Server::new();
        server
            .workspace_roots
            .push(root.to_string_lossy().into_owned());
        server.docs.insert(
            uri.clone(),
            Document::new(path, src.to_string(), 7),
        );

        let expr_start = src.find("left && right").unwrap();
        let extracted = code_actions_for(
            &server,
            &uri,
            src,
            expr_start,
            expr_start + "left && right".len(),
        );
        assert!(extracted.contains("\"title\":\"Extract binding\""), "{extracted}");
        assert!(extracted.contains("\"title\":\"Extract function\""), "{extracted}");
        assert!(extracted.contains("\"kind\":\"refactor.extract\""), "{extracted}");
        assert!(
            extracted.contains(&format!(
                r#""textDocument":{{"uri":"{}","version":7}}"#,
                json_escape(&uri)
            )),
            "{extracted}"
        );

        let use_start = src.rfind("total").unwrap();
        let inlined = code_actions_for(
            &server,
            &uri,
            src,
            use_start,
            use_start + "total".len(),
        );
        assert!(inlined.contains("\"title\":\"Inline `total`\""), "{inlined}");
        assert!(inlined.contains("\"kind\":\"refactor.inline\""), "{inlined}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn code_actions_reject_effects_reordering_and_possible_traps() {
        let effectful = "fn next() -> Int { print(\"effect\"); return 1 }\nfn run() {\n    value :: next()\n    print(\"between\")\n    print(value)\n}\n";
        let effect_root = std::env::temp_dir().join(format!(
            "jet-lsp-effect-refactor-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&effect_root);
        std::fs::create_dir_all(&effect_root).unwrap();
        let effect_path = effect_root.join("main.jet").to_string_lossy().into_owned();
        std::fs::write(&effect_path, effectful).unwrap();
        let effect_uri = path_to_uri(&effect_path);
        let mut effect_server = Server::new();
        effect_server
            .workspace_roots
            .push(effect_root.to_string_lossy().into_owned());
        effect_server.docs.insert(
            effect_uri.clone(),
            Document::new(effect_path, effectful.to_string(), 1),
        );

        let call = effectful.find("next()\n").unwrap();
        let extraction = code_actions_for(
            &effect_server,
            &effect_uri,
            effectful,
            call,
            call + "next()".len(),
        );
        assert!(!extraction.contains("Extract binding"), "{extraction}");
        assert!(!extraction.contains("Extract function"), "{extraction}");
        let use_site = effectful.rfind("value").unwrap();
        let reordered = code_actions_for(
            &effect_server,
            &effect_uri,
            effectful,
            use_site,
            use_site + "value".len(),
        );
        assert!(!reordered.contains("Inline `value`"), "{reordered}");
        let _ = std::fs::remove_dir_all(effect_root);

        let partial =
            "fn run(left: Int, right: Int) {\n    value :: left / right\n    print(value)\n}\n";
        let partial_root = std::env::temp_dir().join(format!(
            "jet-lsp-partial-refactor-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&partial_root);
        std::fs::create_dir_all(&partial_root).unwrap();
        let partial_path = partial_root.join("main.jet").to_string_lossy().into_owned();
        std::fs::write(&partial_path, partial).unwrap();
        let partial_uri = path_to_uri(&partial_path);
        let mut partial_server = Server::new();
        partial_server
            .workspace_roots
            .push(partial_root.to_string_lossy().into_owned());
        partial_server.docs.insert(
            partial_uri.clone(),
            Document::new(partial_path, partial.to_string(), 1),
        );
        let division = partial.find("left / right").unwrap();
        let extraction = code_actions_for(
            &partial_server,
            &partial_uri,
            partial,
            division,
            division + "left / right".len(),
        );
        assert!(!extraction.contains("Extract binding"), "{extraction}");
        assert!(!extraction.contains("Extract function"), "{extraction}");
        let use_site = partial.rfind("value").unwrap();
        let inlined = code_actions_for(
            &partial_server,
            &partial_uri,
            partial,
            use_site,
            use_site + "value".len(),
        );
        assert!(!inlined.contains("Inline `value`"), "{inlined}");
        let _ = std::fs::remove_dir_all(partial_root);

        let clock =
            "fn run(left: Clock, right: Clock) {\n    same :: left == right\n    print(same)\n}\n";
        let clock_root = std::env::temp_dir().join(format!(
            "jet-lsp-clock-refactor-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&clock_root);
        std::fs::create_dir_all(&clock_root).unwrap();
        let clock_path = clock_root.join("main.jet").to_string_lossy().into_owned();
        std::fs::write(&clock_path, clock).unwrap();
        let clock_uri = path_to_uri(&clock_path);
        let mut clock_server = Server::new();
        clock_server
            .workspace_roots
            .push(clock_root.to_string_lossy().into_owned());
        clock_server.docs.insert(
            clock_uri.clone(),
            Document::new(clock_path, clock.to_string(), 1),
        );
        let equality = clock.find("left == right").unwrap();
        let extraction = code_actions_for(
            &clock_server,
            &clock_uri,
            clock,
            equality,
            equality + "left == right".len(),
        );
        assert!(!extraction.contains("Extract binding"), "{extraction}");
        assert!(!extraction.contains("Extract function"), "{extraction}");
        let use_site = clock.rfind("same").unwrap();
        let inlined = code_actions_for(
            &clock_server,
            &clock_uri,
            clock,
            use_site,
            use_site + "same".len(),
        );
        assert!(!inlined.contains("Inline `same`"), "{inlined}");
        let _ = std::fs::remove_dir_all(clock_root);
    }

    #[test]
    fn code_action_imports_one_unique_workspace_symbol() {
        let root = std::env::temp_dir().join(format!(
            "jet-lsp-import-action-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let helper = root.join("helper.jet");
        let src = "fn run() { print(answer()) }\n";
        std::fs::write(&main, src).unwrap();
        std::fs::write(&helper, "pub fn answer() -> Int { return 42 }\n").unwrap();

        let path = main.to_string_lossy().into_owned();
        let uri = path_to_uri(&path);
        let mut server = Server::new();
        server
            .workspace_roots
            .push(root.to_string_lossy().into_owned());
        server.docs.insert(
            uri.clone(),
            Document::new(path, src.to_string(), 3),
        );
        let start = src.find("answer").unwrap();
        let actions = code_actions_for(
            &server,
            &uri,
            src,
            start,
            start + "answer".len(),
        );
        assert!(actions.contains("\"title\":\"Import `helper`\""), "{actions}");
        assert!(actions.contains(r#""newText":"use helper\n""#), "{actions}");
        assert!(actions.contains("\"kind\":\"quickfix\""), "{actions}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn code_action_import_excludes_a_dirty_dependency_overlay() {
        let root = std::env::temp_dir().join(format!(
            "jet-lsp-dirty-import-action-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let helper = root.join("helper.jet");
        let src = "fn run() { print(answer()) }\n";
        let saved_helper = "pub fn answer() -> Int { return 42 }\n";
        let unsaved_helper = "fn answer() -> Int { return 42 }\n";
        std::fs::write(&main, src).unwrap();
        std::fs::write(&helper, saved_helper).unwrap();

        let path = main.to_string_lossy().into_owned();
        let helper_path = helper.to_string_lossy().into_owned();
        let uri = path_to_uri(&path);
        let helper_uri = path_to_uri(&helper_path);
        let mut server = Server::new();
        server
            .workspace_roots
            .push(root.to_string_lossy().into_owned());
        server
            .docs
            .insert(uri.clone(), Document::new(path, src.to_string(), 1));
        server.docs.insert(
            helper_uri,
            Document::new(helper_path, unsaved_helper.to_string(), 2),
        );
        let start = src.find("answer").unwrap();
        let actions = code_actions_for(
            &server,
            &uri,
            src,
            start,
            start + "answer".len(),
        );
        assert!(!actions.contains("Import `helper`"), "{actions}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn e2702_lsp_diagnostic_carries_the_closed_compiler_projection() {
        let diagnostic = Diagnostic::crypto_misuse(
            "nonce has 1 byte; this operation requires exactly 24".into(),
            "pass a 24-byte nonce".into(),
            Span::new(4, 7),
            crate::Diagnostics::CryptoMisuseReason::NonceLength,
            "xchacha20poly1305_seal",
            "exactly 24",
            1,
        );
        let mut server = Server::new();
        server.workspace_roots.push("/workspace".into());
        let file = workspace_relative_diagnostic_path(&server, "/workspace/src/main.jet");
        assert_eq!(file, "src/main.jet");
        let src = "xxxx[0]";
        let compiler_data = diagnostic.structured_json(&file, src).unwrap();
        let json = diagnostic_json(&diagnostic, &file, src);
        assert_eq!(
            json,
            concat!(
                "{\"range\":{\"start\":{\"line\":0,\"character\":4},",
                "\"end\":{\"line\":0,\"character\":7}},\"severity\":1,",
                "\"code\":\"E2702\",\"source\":\"jet\",\"message\":\"crypto API misuse\", ",
                "\"data\":{\"schema\":\"jet.diagnostic/v1\",\"code\":\"E2702\",",
                "\"class\":\"user\",\"severity\":\"error\",\"phase\":\"sema\",",
                "\"what\":\"crypto API misuse\",",
                "\"why\":\"nonce has 1 byte; this operation requires exactly 24\",",
                "\"fix\":\"pass a 24-byte nonce\",\"reason\":\"nonce_length\",",
                "\"operation\":\"xchacha20poly1305_seal\",\"expected\":\"exactly 24\",",
                "\"actual\":1,\"primarySpan\":{\"file\":\"src/main.jet\",",
                "\"start\":4,\"end\":7,\"line\":1,\"col\":5},\"relatedSpans\":[]}}"
            )
        );
        assert!(json.ends_with(&format!(", \"data\":{compiler_data}}}")), "{json}");
        assert!(!json.contains("backend"), "{json}");
    }

    #[test]
    fn e2702_lsp_omits_bounds_that_do_not_apply() {
        let diagnostic = Diagnostic::crypto_misuse_fact(
            "safe envelopes manage nonces".into(),
            "remove the raw nonce".into(),
            Span::new(2, 7),
            crate::Diagnostics::CryptoMisuseReason::RawNonce,
            "seal",
        );
        let json = diagnostic_json(&diagnostic, "src/main.jet", "xxnonce");
        assert!(json.contains("\"reason\":\"raw_nonce\""), "{json}");
        assert!(json.contains("\"operation\":\"seal\""), "{json}");
        assert!(!json.contains("\"expected\":"), "{json}");
        assert!(!json.contains("\"actual\":"), "{json}");
    }

    #[test]
    fn e2702_lsp_never_exposes_an_absolute_path_without_a_workspace_root() {
        let server = Server::new();
        let file = workspace_relative_diagnostic_path(
            &server,
            "/private/attacker-controlled/project/src/main.jet",
        );
        assert_eq!(file, "main.jet");
        assert!(!file.starts_with('/'));
        assert_eq!(workspace_relative_diagnostic_path(&server, "src/main.jet"), "src/main.jet");
    }

    #[test]
    fn cancellation_suppresses_an_in_flight_success_response() {
        let cancelled = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let rendezvous = Arc::new(std::sync::Barrier::new(2));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_rendezvous = Arc::clone(&rendezvous);
        let canceller = std::thread::spawn(move || {
            worker_rendezvous.wait();
            lock_cancelled(&worker_cancelled).insert("7".to_string());
            worker_rendezvous.wait();
        });

        let response = cancellable_response(&cancelled, "7", &JsonValue::Number(7), || {
            rendezvous.wait();
            rendezvous.wait();
            Some(response(&JsonValue::Number(7), "true"))
        })
        .expect("cancel response");
        canceller.join().unwrap();

        assert!(response.contains(r#""code":-32800"#), "{response}");
        assert!(!response.contains(r#""result":true"#), "{response}");
    }

    #[test]
    fn range_formatting_leaves_unselected_lines_unchanged() {
        let source = "fn one(){\nprint(1)\n}\nfn two(){\nprint(2)\n}\n";
        let requested = LspRange {
            start: LspPos {
                line: 0,
                character: 0,
            },
            end: LspPos {
                line: 3,
                character: 0,
            },
        };
        let (range, new_text) =
            format_requested_lines(source, requested).expect("range formatting edit");
        let edited = apply_lsp_edit(source, range, None, &new_text).expect("valid edit");

        assert!(edited.starts_with("fn one() {\n    print(1)\n}\n"), "{edited}");
        assert!(edited.ends_with("fn two(){\nprint(2)\n}\n"), "{edited}");
        assert_eq!(range.start, requested.start);
        assert_eq!(range.end, requested.end);
    }

    #[test]
    fn incremental_lsp_query_diagnostics_match_fresh_check_bytes() {
        let path = "/tmp/lsp_incremental_diagnostic_parity.jet";
        let before = "fn alpha() -> Int { return 1 }\nfn beta() -> Int { return 2 }\n";
        let mut doc = Document::new(path.to_string(), before.to_string(), 1);
        let server = Server::new();
        assert!(server.check(&doc).is_empty());

        let value = before.rfind('2').unwrap();
        let range = LspRange {
            start: byte_offset_to_lsp(before, value),
            end: byte_offset_to_lsp(before, value + 1),
        };
        assert!(doc.apply_range_edit(range, Some(1), "\"wrong\""));
        let incremental = server.check(&doc);
        let fresh = super::super::Check::check_document(path, &doc.text);

        assert_eq!(
            crate::render_all_json(path, &doc.text, &incremental),
            crate::render_all_json(path, &doc.text, &fresh)
        );
        assert!(!incremental.is_empty());
    }

    #[cfg(unix)]
    #[derive(Debug)]
    struct ShortTestDir(std::path::PathBuf);

    #[cfg(unix)]
    impl ShortTestDir {
        fn reserve(base: &std::path::Path, prefix: &str, attempts: usize) -> io::Result<Self> {
            for attempt in 0..attempts {
                let path = base.join(format!("{prefix}-{attempt}"));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Ok(Self(path)),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        return Err(io::Error::new(
                            error.kind(),
                            format!("failed to create {}: {error}", path.display()),
                        ));
                    }
                }
            }
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "failed to reserve a unique {prefix} directory under {} after {attempts} attempts",
                    base.display()
                ),
            ))
        }
    }

    #[cfg(unix)]
    impl Drop for ShortTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn json_rpc_envelope_rejects_malformed_or_ambiguous_requests() {
        assert!(parse_rpc_message(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#
        )
        .is_ok());
        for (raw, code) in [
            (r#"{"jsonrpc":"2.0","method":"x","method":"y"}"#, -32700),
            (r#"{"jsonrpc":"1.0","method":"x"}"#, -32600),
            (r#"{"jsonrpc":"2.0","id":1.5,"method":"x"}"#, -32600),
            (r#"{"jsonrpc":"2.0","method":"x","params":1}"#, -32600),
            (r#"["not","a","request"]"#, -32600),
        ] {
            assert_eq!(parse_rpc_message(raw).unwrap_err().0, code, "{raw}");
        }
    }

    #[test]
    fn lsp_rejects_oversized_frame_before_reading_a_body() {
        let frame = format!("Content-Length: {}\r\n\r\n", MAX_PROTOCOL_MESSAGE_BYTES + 1);
        let error = read_message(&mut std::io::Cursor::new(frame)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "protocol message exceeds the 1048576-byte limit"
        );
    }

    #[test]
    fn workspace_root_initialization_uses_protocol_precedence() {
        let mut server = Server::new();
        let params = parse_json(
            r#"{"workspaceFolders":[{"uri":"file:///tmp/folder","name":"folder"}],"rootUri":"file:///tmp/uri","rootPath":"/tmp/path"}"#,
        )
        .unwrap();
        configure_workspace_roots(&mut server, Some(&params));
        assert_eq!(server.workspace_roots, vec![normalize_path("/tmp/folder")]);

        let params = parse_json(r#"{"rootUri":"file:///tmp/uri","rootPath":"/tmp/path"}"#)
            .unwrap();
        configure_workspace_roots(&mut server, Some(&params));
        assert_eq!(server.workspace_roots, vec![normalize_path("/tmp/uri")]);
    }

    #[test]
    fn file_uri_percent_encoding_rejects_hostile_escapes() {
        assert_eq!(uri_to_path("file:///tmp/a%20b"), "/tmp/a b");
        assert_eq!(path_to_uri("/tmp/a b#c%"), "file:///tmp/a%20b%23c%25");
        for uri in ["file:///tmp/%", "file:///tmp/%GG", "file:///tmp/%FF"] {
            assert_eq!(uri_to_path(uri), "", "malformed URI must be rejected: {uri}");
        }
    }

    #[test]
    fn shared_queries_preserve_unrelated_roots() {
        let root = std::env::temp_dir().join(format!("jet-lsp-queries-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let a_path = root.join("query-a.jet").to_string_lossy().into_owned();
        let b_path = root.join("query-b.jet").to_string_lossy().into_owned();
        let server = Server::new();
        let mut a = Document::new(a_path, "fn run() {}\n".into(), 1);
        let b = Document::new(b_path.clone(), "fn helper() {}\n".into(), 1);

        assert!(server.check(&a).is_empty());
        assert!(server.check(&a).is_empty());
        assert!(server.check(&b).is_empty());
        a.replace_text("fn run() { print(\"changed\") }\n".into());
        assert!(server.check(&a).is_empty());
        assert!(server.check(&b).is_empty());

        let queries = server.queries.borrow();
        let stats = queries.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.recomputes, 3);
        assert_eq!(stats.live_inputs, 4);
        assert_eq!(stats.live_memos, 2);
        assert_eq!(stats.live_query_counters, 2);
        assert_eq!(
            queries.recompute_count(&QueryKey::for_file(
                "checked.lsp",
                FileKey::new(b_path)
            )),
            1,
            "an unrelated root must remain warm"
        );
        drop(queries);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn open_dependency_overlay_invalidates_importer() {
        let root = std::env::temp_dir().join(format!("jet-lsp-overlay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main_path = root.join("main.jet");
        let dependency_path = root.join("b.jet");
        let main_source = "module b;\nfn run() -> Int { return b.value() }\n";
        std::fs::write(&dependency_path, "pub fn value() -> Int { return 1 }\n").unwrap();
        let main_uri = path_to_uri(&main_path.to_string_lossy());
        let dependency_uri = path_to_uri(&dependency_path.to_string_lossy());
        let mut server = Server::new();
        server.docs.insert(
            main_uri.clone(),
            Document::new(main_path.to_string_lossy().into_owned(), main_source.into(), 1),
        );
        server.docs.insert(
            dependency_uri.clone(),
            Document::new(
                dependency_path.to_string_lossy().into_owned(),
                "pub fn value() -> String { return \"unsaved\" }\n".into(),
                1,
            ),
        );

        let broken = server.check(server.docs.get(&main_uri).unwrap());
        assert!(!broken.is_empty(), "the importer must see the unsaved dependency");
        server
            .docs
            .get_mut(&dependency_uri)
            .unwrap()
            .replace_text("pub fn value() -> Int { return 2 }\n".into());
        let repaired = server.check(server.docs.get(&main_uri).unwrap());

        assert!(repaired.is_empty(), "{repaired:#?}");
        assert_eq!(
            server.queries.borrow().recompute_count(&QueryKey::for_file(
                "checked.lsp",
                FileKey::new(main_path.to_string_lossy())
            )),
            2
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn short_socket_directory_reservation_is_bounded_and_skips_collisions() {
        let root = ShortTestDir::reserve(
            std::path::Path::new("/tmp"),
            &format!("jet-lsp-reservation-{}", std::process::id()),
            64,
        )
        .expect("failed to reserve collision-test directory");
        std::fs::create_dir(root.0.join("slot-0")).unwrap();
        let reserved = ShortTestDir::reserve(&root.0, "slot", 2).unwrap();
        assert_eq!(reserved.0.file_name().unwrap(), "slot-1");

        std::fs::create_dir(root.0.join("full-0")).unwrap();
        let error = ShortTestDir::reserve(&root.0, "full", 1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(error.to_string().contains("after 1 attempts"));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_scan_ignores_non_regular_jet_entries() {
        let root = ShortTestDir::reserve(
            std::path::Path::new("/tmp"),
            &format!("jet-lsp-{}", std::process::id()),
            64,
        )
        .expect("failed to reserve short Unix socket test directory");
        let source = root.0.join("main.jet");
        std::fs::write(&source, "fn run() {}\n").unwrap();
        let socket =
            std::os::unix::net::UnixListener::bind(root.0.join("blocked.jet")).unwrap();

        let mut files = Vec::new();
        collect_jet_files(&root.0, &mut files);
        assert_eq!(files, vec![source]);

        drop(socket);
    }

    #[test]
    fn workspace_index_skips_internal_parts_until_explicit_import() {
        let root = std::env::temp_dir().join(format!(
            "jet-lsp-project-parts-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.jet");
        let internal = root.join("bench.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();
        std::fs::write(&internal, "module _bench { }\nfn hidden_probe() {}\n").unwrap();

        let mut server = Server::new();
        server.workspace_roots.push(normalize_path_buf(&root));
        let entry = entry.to_string_lossy();
        let sources = workspace_sources(&server, Some(&entry));
        assert!(!sources.iter().any(|(path, _)| path.ends_with("bench.jet")));

        let uri = path_to_uri(&entry);
        server.docs.insert(
            uri,
            Document::new(
                entry.to_string(),
                "use project._bench;\nfn run() {}\n".to_string(),
                1,
            ),
        );
        let sources = workspace_sources(&server, Some(&entry));
        assert!(sources.iter().any(|(path, _)| path.ends_with("bench.jet")));
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

fn workspace_relative_diagnostic_path(server: &Server, path: &str) -> String {
    let normalized = normalize_path(path);
    let Some(root) = workspace_root_for_path(server, &normalized) else {
        let path = std::path::Path::new(&normalized);
        return if path.is_absolute() {
            path.file_name().map_or_else(
                || "<source>".to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        } else {
            normalized
        };
    };
    std::path::Path::new(&normalized)
        .strip_prefix(&root)
        .unwrap_or(std::path::Path::new(&normalized))
        .to_string_lossy()
        .replace('\\', "/")
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
        } else if trimmed.starts_with("#Test") {
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
        let Some(rest) = percent_decode(rest) else {
            return String::new();
        };
        if cfg!(windows) {
            rest.trim_start_matches('/').replace('/', "\\")
        } else {
            rest
        }
    } else {
        uri.to_string()
    }
}

fn path_to_uri(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("file://{encoded}")
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'%' {
            return None;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
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
