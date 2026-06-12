//! LSP v0 (M6 phase 4): diagnostics, S14 quick-fixes, formatting.
//!
//! Hand-rolled JSON-RPC over stdio (invariant I6 — no serde in the compiler).

use crate::diag::{Diagnostic, Severity, Span, TextEdit};
use crate::sema::CompileMode;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

// --- minimal JSON (parse only what LSP needs) --------------------------------

#[derive(Debug, Clone)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

fn parse_json(text: &str) -> Result<JsonValue, ()> {
    let mut p = JsonParser { s: text, i: 0 };
    let v = p.value()?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err(());
    }
    Ok(v)
}

struct JsonParser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    fn value(&mut self) -> Result<JsonValue, ()> {
        self.skip_ws();
        match self.peek() {
            Some('n') => {
                self.expect_literal("null")?;
                Ok(JsonValue::Null)
            }
            Some('t') => {
                self.expect_literal("true")?;
                Ok(JsonValue::Bool(true))
            }
            Some('f') => {
                self.expect_literal("false")?;
                Ok(JsonValue::Bool(false))
            }
            Some('"') => Ok(JsonValue::String(self.string()?)),
            Some('[') => {
                self.bump();
                let mut arr = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.bump();
                    return Ok(JsonValue::Array(arr));
                }
                loop {
                    arr.push(self.value()?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Array(arr))
            }
            Some('{') => {
                self.bump();
                let mut obj = HashMap::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(JsonValue::Object(obj));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if self.bump() != Some(':') {
                        return Err(());
                    }
                    obj.insert(key, self.value()?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Object(obj))
            }
            Some(c) if c == '-' || c.is_ascii_digit() => Ok(JsonValue::Number(self.number()?)),
            _ => Err(()),
        }
    }

    fn expect_literal(&mut self, lit: &str) -> Result<(), ()> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn string(&mut self) -> Result<String, ()> {
        if self.bump() != Some('"') {
            return Err(());
        }
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.bump();
                return Ok(out);
            }
            if c == '\\' {
                self.bump();
                let esc = self.bump().ok_or(())?;
                out.push(match esc {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let hex: String = self.s[self.i..].chars().take(4).collect();
                        if hex.len() != 4 {
                            return Err(());
                        }
                        self.i += 4;
                        char::from_u32(u32::from_str_radix(&hex, 16).map_err(|_| ())?)
                            .ok_or(())?
                    }
                    _ => return Err(()),
                });
            } else {
                self.bump();
                out.push(c);
            }
        }
        Err(())
    }

    fn number(&mut self) -> Result<i64, ()> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.bump();
        }
        while matches!(self.peek(), Some('0'..='9')) {
            self.bump();
        }
        self.s[start..self.i].parse().map_err(|_| ())
    }
}

fn json_get<'a>(v: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match v {
        JsonValue::Object(m) => m.get(key),
        _ => None,
    }
}

fn json_str(v: &JsonValue) -> Option<&str> {
    match v {
        JsonValue::String(s) => Some(s),
        _ => None,
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// --- LSP positions (UTF-16 code units) ---------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LspPos {
    line: u32,
    character: u32,
}

#[derive(Clone, Copy, Debug)]
struct LspRange {
    start: LspPos,
    end: LspPos,
}

fn byte_span_to_range(src: &str, span: Span) -> LspRange {
    LspRange {
        start: byte_offset_to_lsp(src, span.start),
        end: byte_offset_to_lsp(src, span.end),
    }
}

fn byte_offset_to_lsp(src: &str, offset: usize) -> LspPos {
    let offset = offset.min(src.len());
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let line_text = &src[line_start..offset];
    let character = line_text.encode_utf16().count() as u32;
    LspPos { line, character }
}

fn full_document_range(src: &str) -> LspRange {
    let end = byte_offset_to_lsp(src, src.len());
    LspRange {
        start: LspPos {
            line: 0,
            character: 0,
        },
        end,
    }
}

// --- document state ----------------------------------------------------------

struct Document {
    path: String,
    text: String,
}

struct Server {
    docs: HashMap<String, Document>,
    shutdown: bool,
}

impl Server {
    fn new() -> Self {
        Server {
            docs: HashMap::new(),
            shutdown: false,
        }
    }

    fn check(&self, doc: &Document) -> Vec<Diagnostic> {
        crate::check_document(&doc.path, &doc.text)
    }
}

// --- JSON-RPC ----------------------------------------------------------------

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
                let resp = handle_request(&mut server, method, params, id.as_ref().unwrap());
                if let Some(resp) = resp {
                    write_message(&mut stdout, &resp)?;
                }
            } else {
                handle_notification(&mut server, method, params, &mut stdout)?;
            }
        }

        if server.shutdown {
            break;
        }
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

fn handle_request(
    server: &mut Server,
    method: &str,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    match method {
        "initialize" => Some(initialize_response(id)),
        "shutdown" => {
            server.shutdown = true;
            Some(response(id, r#"null"#))
        }
        "textDocument/codeAction" => code_action_response(server, params, id),
        "textDocument/formatting" => format_response(server, params, id),
        _ => Some(response(id, r#"null"#)),
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
        "textDocument/didOpen" => publish_after_change(server, params, stdout),
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
    "codeActionProvider": true
  },
  "serverInfo": { "name": "jet", "version": "0.1.0" }
}"#;
    response(id, result)
}

fn response(id: &JsonValue, result_json: &str) -> String {
    let id_json = serialize_id(id);
    format!(r#"{{"jsonrpc":"2.0","id":{},"result":{}}}"#, id_json, result_json)
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

    if let Some(doc) = server.docs.get(&uri) {
        let diags = server.check(doc);
        let notif = publish_diagnostics(&uri, &doc.text, &diags);
        write_message(stdout, &notif)?;
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

fn range_json(r: LspRange) -> String {
    format!(
        r#"{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}}"#,
        r.start.line, r.start.character, r.end.line, r.end.character
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
    let diags = server.check(doc);
    let mut actions = String::new();
    let mut n = 0usize;
    for d in &diags {
        if let Some(edit) = &d.edit {
            if n > 0 {
                actions.push(',');
            }
            actions.push_str(&code_action_json(uri, &doc.text, d, edit));
            n += 1;
        }
    }
    Some(response(id, &format!("[{}]", actions)))
}

fn code_action_json(uri: &str, src: &str, d: &Diagnostic, edit: &TextEdit) -> String {
    let range = byte_span_to_range(src, edit.span);
    let title = d.fix.clone();
    format!(
        r#"{{"title":"{}","kind":"quickfix","edit":{{"changes":{{"{}":[{{"range":{},"newText":"{}"}}]}}}}}}"#,
        json_escape(&title),
        json_escape(uri),
        range_json(range),
        json_escape(&edit.new_text)
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

fn canonical_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        normalize_path(&p)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        normalize_path(&cwd.join(p))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Check one document (disk path + in-memory text). Used by LSP and tests.
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    let abs = canonical_path(path);
    match crate::loader::load_entry_with_overlay(path, Some((&abs, text)), true) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            diags.extend(crate::sema::check_bundle(&mut bundle, CompileMode::Check));
            diags
        }
        Err(diags) => diags,
    }
}

/// Apply a teaching edit to source text (for scripted LSP tests).
pub fn apply_edit(src: &str, edit: &TextEdit) -> String {
    let mut out = String::new();
    out.push_str(&src[..edit.span.start.min(src.len())]);
    out.push_str(&edit.new_text);
    out.push_str(&src[edit.span.end.min(src.len())..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teaching_edit_from_let() {
        let src = "fn main() {\n    let x = 1;\n}\n";
        let diags = check_document("test.jet", src);
        let e0009 = diags.iter().find(|d| d.code == "E0009").expect("E0009");
        let edit = e0009.edit.as_ref().expect("edit");
        assert_eq!(edit.new_text, "val");
        let fixed = apply_edit(src, edit);
        assert!(fixed.contains("val x = 1"));
        assert!(!fixed.contains("let x"));
    }
}
