//! JSON-RPC transport over stdio + request/notification dispatch + handlers.

use crate::AST::ProgramBundle;
use crate::Diagnostics::{Diagnostic, Severity, Span};
use crate::Lexer::Token;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use super::Check::{check_document, check_document_with_bundle, collect_fixes, Fix};
use super::Completion::compute_completions;
use super::Features::{
    compute_definition, compute_hover, compute_references, compute_rename, encode_semantic_tokens,
    format_inlay_hints,
};
use super::JSON::{json_escape, json_get, json_int, json_str, parse_json, JsonValue};
use super::Position::{
    byte_span_to_range, full_document_range, lsp_pos_to_offset, range_json, LspPos,
};
use super::SymbolDB::{build_symbol_db, InlayHint, SymbolDB};

// ── Document state ────────────────────────────────────────────────────────────

struct Document {
    path: String,
    text: String,
}

struct Server {
    docs: HashMap<String, Document>,
    /// URIs of documents that changed since last diagnostic publish (D-LSP3).
    dirty: std::collections::HashSet<String>,
    /// D-LSP4: diagnostic cache keyed by path → (source-hash, diagnostics).
    /// RefCell allows mutation through &self so callers can hold &Document refs.
    diag_cache: std::cell::RefCell<HashMap<String, (u64, Vec<Diagnostic>)>>,
    shutdown: bool,
}

/// FNV-1a 64-bit hash of a string — good enough for source-change detection.
fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

impl Server {
    fn new() -> Self {
        Server {
            docs: HashMap::new(),
            dirty: std::collections::HashSet::new(),
            diag_cache: std::cell::RefCell::new(HashMap::new()),
            shutdown: false,
        }
    }

    /// D-LSP4: return diagnostics, re-using cached results when source is unchanged.
    fn check(&self, doc: &Document) -> Vec<Diagnostic> {
        let h = hash_str(&doc.text);
        {
            let cache = self.diag_cache.borrow();
            if let Some((cached_h, cached)) = cache.get(&doc.path) {
                if *cached_h == h {
                    return cached.clone();
                }
            }
        }
        let diags = check_document(&doc.path, &doc.text);
        self.diag_cache
            .borrow_mut()
            .insert(doc.path.clone(), (h, diags.clone()));
        diags
    }

    fn check_with_bundle(&self, doc: &Document) -> (Vec<Diagnostic>, Option<ProgramBundle>) {
        check_document_with_bundle(&doc.path, &doc.text)
    }

    fn lex(&self, doc: &Document) -> Vec<Token> {
        let (toks, _) = crate::Lexer::lex(&doc.text);
        toks
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
        "initialize" => Some(initialize_response(id)),
        "shutdown" => {
            server.shutdown = true;
            Some(response(id, "null"))
        }
        "textDocument/codeAction" => code_action_response(server, params, id),
        "textDocument/formatting" => format_response(server, params, id),
        "textDocument/rangeFormatting" => format_response(server, params, id),
        "textDocument/completion" => completion_response(server, params, id),
        "textDocument/hover" => hover_response(server, params, id),
        "textDocument/definition" => definition_response(server, params, id),
        "textDocument/references" => references_response(server, params, id),
        "textDocument/rename" => rename_response(server, params, id),
        "textDocument/semanticTokens/full" => semantic_tokens_response(server, params, id),
        "textDocument/inlayHint" => inlay_hint_response(server, params, id),
        _ => Some(response(id, "null")),
    };
    if let Some(t0) = started {
        record_lsp_latency(method, t0.elapsed().as_micros());
    }
    out
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
                server.docs.remove(uri);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn initialize_response(id: &JsonValue) -> String {
    let result = r#"{
  "capabilities": {
    "textDocumentSync": 1,
    "documentFormattingProvider": true,
    "documentRangeFormattingProvider": true,
    "codeActionProvider": true,
    "completionProvider": {
      "triggerCharacters": ["."],
      "resolveProvider": false
    },
    "hoverProvider": true,
    "definitionProvider": true,
    "referencesProvider": true,
    "renameProvider": true,
    "semanticTokensProvider": {
      "legend": {
        "tokenTypes": [
          "keyword","type","function","variable","parameter",
          "property","enumMember","string","number","comment",
          "operator","namespace"
        ],
        "tokenModifiers": ["declaration","readonly"]
      },
      "full": true
    },
    "inlayHintProvider": true
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
        server.docs.insert(
            uri.clone(),
            Document {
                path,
                text: text.to_string(),
            },
        );
    } else if let Some(changes) = json_get(params, "contentChanges") {
        if let JsonValue::Array(arr) = changes {
            if let Some(JsonValue::Object(chg)) = arr.first() {
                if let Some(text) = chg.get("text").and_then(json_str) {
                    server.docs.insert(
                        uri.clone(),
                        Document {
                            path,
                            text: text.to_string(),
                        },
                    );
                }
            }
        }
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
        json_escape(d.code),
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
    let fixes = collect_fixes(&doc_path_for(uri), &doc.text);
    let mut actions = String::new();
    for (n, fix) in fixes.iter().enumerate() {
        if n > 0 {
            actions.push(',');
        }
        actions.push_str(&code_action_json(uri, &doc.text, fix));
    }
    Some(response(id, &format!("[{}]", actions)))
}

/// The path `collect_fixes` should check for a `file://` URI (falls back to the
/// raw URI so non-file documents still work).
fn doc_path_for(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
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

    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
        None => SymbolDB::new(),
    };

    let items = compute_completions(&db, &doc.text, offset, &doc.path);
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
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
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
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
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
        None => Some(response(id, "null")),
    }
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
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
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
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
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
    let data_str: Vec<String> = data.iter().map(|n| n.to_string()).collect();
    Some(response(
        id,
        &format!(r#"{{"data":[{}]}}"#, data_str.join(",")),
    ))
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

    let (diags, bundle) = server.check_with_bundle(doc);

    // Build type-annotation hints from the symbol DB.
    let mut hints: Vec<InlayHint> = match bundle {
        Some(b) => {
            let db = build_symbol_db(&b);
            db.inlay_hints_for(&doc.path).into_iter().cloned().collect()
        }
        None => Vec::new(),
    };

    // D-LSP8: add clone-site hints for L0201 diagnostics.
    for d in &diags {
        if d.code == "L0201" {
            if let Some(span) = d.span {
                hints.push(InlayHint {
                    span,
                    module_path: doc.path.clone(),
                    label: ".clone()".to_string(),
                });
            }
        }
    }

    let hint_refs: Vec<&InlayHint> = hints.iter().collect();
    let json = format_inlay_hints(&hint_refs, &doc.text);
    Some(response(id, &json))
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
