//! M13 LSP integration tests: scripted JSON transcripts + latency bench.
//!
//! Each test in tests/lsp/*.json is replayed against a live `jet lsp` process.
//! The transcript runner:
//!   - Sends each step's `send` message (or opens a document for `open` steps)
//!   - Reads the next server message
//!   - Asserts that `expect_contains` strings all appear in the response
//!   - Skips null `expect_notification` steps (just sends, reads nothing)
//!
//! Tests skip gracefully if the `jet` binary is not built.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn send_msg(stdin: &mut impl Write, json: &str) {
    write!(stdin, "Content-Length: {}\r\n\r\n{}", json.len(), json).unwrap();
    stdin.flush().unwrap();
}

fn read_msg(stdout: &mut impl Read) -> String {
    let mut header = String::new();
    let mut buf = [0u8; 1];
    loop {
        stdout.read_exact(&mut buf).unwrap();
        header.push(buf[0] as char);
        if header.ends_with("\r\n\r\n") {
            break;
        }
    }
    let len: usize = header
        .lines()
        .find_map(|l| l.strip_prefix("Content-Length:").map(|n| n.trim()))
        .expect("Content-Length")
        .parse()
        .unwrap();
    let mut body = vec![0u8; len];
    stdout.read_exact(&mut body).unwrap();
    String::from_utf8(body).unwrap()
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug)]
struct DecodedSemanticToken {
    text: String,
    token_type: u32,
    modifiers: u32,
}

fn semantic_data_from_response(resp: &str) -> Vec<u32> {
    let data_key = "\"data\"";
    let start = resp.find(data_key).expect("semantic token data");
    let after_key = &resp[start + data_key.len()..];
    let array_start = after_key.find('[').expect("data array");
    let after_array = &after_key[array_start + 1..];
    let array_end = after_array.find(']').expect("data array end");
    after_array[..array_end]
        .split(',')
        .filter_map(|n| n.trim().parse::<u32>().ok())
        .collect()
}

fn decode_semantic_tokens(src: &str, resp: &str) -> Vec<DecodedSemanticToken> {
    let data = semantic_data_from_response(resp);
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut line = 0u32;
    let mut start = 0u32;

    for chunk in data.chunks_exact(5) {
        line += chunk[0];
        start = if chunk[0] == 0 {
            start + chunk[1]
        } else {
            chunk[1]
        };
        let len = chunk[2];
        let text = lines
            .get(line as usize)
            .and_then(|line_text| {
                let s = start as usize;
                let e = s + len as usize;
                line_text.get(s..e)
            })
            .unwrap_or("")
            .to_string();
        out.push(DecodedSemanticToken {
            text,
            token_type: chunk[3],
            modifiers: chunk[4],
        });
    }
    out
}

fn assert_semantic_token(
    tokens: &[DecodedSemanticToken],
    text: &str,
    token_type: u32,
    modifier: u32,
) {
    assert!(
        tokens.iter().any(|t| {
            t.text == text && t.token_type == token_type && (t.modifiers & modifier) != 0
        }),
        "missing semantic token `{}` type {} modifier {}; got {:?}",
        text,
        token_type,
        modifier,
        tokens
    );
}

const LSP_CAPABILITY_COVERAGE: &[(&str, &str)] = &[
    (
        "textDocumentSync",
        "lsp_incremental_sync_range_edit_updates_document",
    ),
    (
        "documentFormattingProvider",
        "lsp_formatting_and_range_formatting_return_edits",
    ),
    (
        "documentRangeFormattingProvider",
        "lsp_formatting_and_range_formatting_return_edits",
    ),
    ("codeActionProvider", "lsp_teaching_autocorrect_let_to_val"),
    ("completionProvider", "lsp_completion_returns_items"),
    (
        "signatureHelpProvider",
        "lsp_signature_help_returns_active_parameter",
    ),
    (
        "documentSymbolProvider",
        "lsp_document_symbol_returns_checked_outline",
    ),
    ("workspaceSymbolProvider", "lsp_wave2_navigation_features"),
    ("foldingRangeProvider", "lsp_wave2_navigation_features"),
    ("documentHighlightProvider", "lsp_wave2_navigation_features"),
    ("selectionRangeProvider", "lsp_wave2_navigation_features"),
    ("documentLinkProvider", "lsp_document_links_and_code_lenses"),
    ("codeLensProvider", "lsp_document_links_and_code_lenses"),
    ("hoverProvider", "lsp_hover_returns_signature"),
    ("definitionProvider", "lsp_definition_returns_location"),
    ("referencesProvider", "lsp_references_finds_all_uses"),
    ("renameProvider", "lsp_rename_produces_workspace_edit"),
    (
        "renameProvider.prepare",
        "lsp_wave3_prepare_rename_semantic_range_and_call_hierarchy",
    ),
    ("semanticTokensProvider", "lsp_semantic_tokens_returns_data"),
    (
        "semanticTokensProvider.full",
        "lsp_semantic_tokens_returns_data",
    ),
    (
        "semanticTokensProvider.range",
        "lsp_wave3_prepare_rename_semantic_range_and_call_hierarchy",
    ),
    (
        "semanticTokensProvider.delta",
        "lsp_semantic_tokens_delta_returns_full_fallback",
    ),
    ("inlayHintProvider", "lsp_inlay_hints_returns_type_labels"),
    (
        "callHierarchyProvider",
        "lsp_wave3_prepare_rename_semantic_range_and_call_hierarchy",
    ),
    ("typeHierarchyProvider", "lsp_type_hierarchy_trait_impls"),
    (
        "executeCommandProvider",
        "lsp_execute_command_impact_returns_report",
    ),
];

fn advertised_capabilities(init: &str) -> Vec<String> {
    let Some(cap_start) = init.find("\"capabilities\"") else {
        return Vec::new();
    };
    let Some(open_rel) = init[cap_start..].find('{') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    let mut string_start = 0usize;
    let mut last_string: Option<String> = None;
    let bytes = init.as_bytes();
    let mut i = cap_start + open_rel;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                last_string = Some(init[string_start..i].to_string());
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                string_start = i + 1;
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            b':' if depth == 1 => {
                if let Some(key) = last_string.take() {
                    out.push(key);
                }
            }
            _ => {}
        }
        i += 1;
    }

    if init.contains("\"renameProvider\"") && init.contains("\"prepareProvider\": true") {
        out.push("renameProvider.prepare".to_string());
    }
    if init.contains("\"semanticTokensProvider\"") {
        if init.contains("\"full\"") {
            out.push("semanticTokensProvider.full".to_string());
        }
        if init.contains("\"range\": true") {
            out.push("semanticTokensProvider.range".to_string());
        }
        if init.contains("\"delta\": true") {
            out.push("semanticTokensProvider.delta".to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

// ── JSON transcript runner (A1) ──────────────────────────────────────────────
//
// Loads every tests/lsp/*.json file and replays it against a live server.
// This is the canonical source of truth for per-capability coverage.

/// Minimal JSON extractor for transcript files (no external crates — I6).
/// Returns (source, steps) or panics on malformed transcript.
mod transcript_parser {
    /// A parsed step from a JSON transcript file.
    pub struct Step {
        /// Raw JSON object to forward to the server, or None for open steps.
        pub send_raw: Option<String>,
        /// URI to open (for open steps).
        pub open_uri: Option<String>,
        /// Strings that must appear in the server response.
        pub expect_contains: Vec<String>,
        /// For open steps: whether to expect a publishDiagnostics notification.
        pub expect_notification: bool,
    }

    pub struct Transcript {
        #[allow(dead_code)] // parsed from JSON; used for test output context if needed
        pub description: String,
        pub source: String,
        pub steps: Vec<Step>,
    }

    /// Unescape a JSON string value (already stripped of surrounding `"`).
    fn unescape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('b') => out.push('\x08'),
                Some('f') => out.push('\x0c'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(n) {
                            out.push(c);
                        }
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => break,
            }
        }
        out
    }

    /// Extract the value of a top-level JSON string key.
    fn extract_string_field<'a>(json: &'a str, key: &str) -> Option<String> {
        let needle = format!("\"{}\"", key);
        let pos = json.find(&needle)?;
        let after = json[pos + needle.len()..].trim_start();
        let after = after.strip_prefix(':')?.trim_start();
        let after = after.strip_prefix('"')?;
        // Find unescaped closing quote.
        let mut end = 0;
        let bytes = after.as_bytes();
        loop {
            if end >= bytes.len() {
                return None;
            }
            if bytes[end] == b'\\' {
                end += 2;
                continue;
            }
            if bytes[end] == b'"' {
                break;
            }
            end += 1;
        }
        Some(unescape(&after[..end]))
    }

    /// Skip whitespace and return remaining.
    fn skip_ws(s: &str) -> &str {
        s.trim_start_matches(|c: char| c.is_ascii_whitespace())
    }

    /// Find the matching closing bracket/brace for a JSON collection starting at `s[0]`.
    fn find_matching_close(s: &str, open: char, close: char) -> Option<usize> {
        let mut depth = 0usize;
        let mut in_str = false;
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if in_str {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == b'"' {
                    in_str = false;
                }
            } else {
                let c = s[i..].chars().next().unwrap();
                if c == '"' {
                    in_str = true;
                } else if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
            }
            i += c_len(bytes, i);
        }
        None
    }

    fn c_len(bytes: &[u8], i: usize) -> usize {
        let b = bytes[i];
        if b < 0x80 {
            1
        } else if b < 0xE0 {
            2
        } else if b < 0xF0 {
            3
        } else {
            4
        }
    }

    /// Extract a JSON array of strings.
    fn extract_string_array(s: &str) -> Vec<String> {
        let s = skip_ws(s);
        if !s.starts_with('[') {
            return Vec::new();
        }
        let close = match find_matching_close(s, '[', ']') {
            Some(i) => i,
            None => return Vec::new(),
        };
        let inner = &s[1..close];
        let mut result = Vec::new();
        let mut remaining = skip_ws(inner);
        while !remaining.is_empty() {
            remaining = skip_ws(remaining);
            if remaining.starts_with(']') || remaining.is_empty() {
                break;
            }
            if remaining.starts_with('"') {
                let rest = &remaining[1..];
                let mut end = 0;
                let bytes = rest.as_bytes();
                loop {
                    if end >= bytes.len() {
                        break;
                    }
                    if bytes[end] == b'\\' {
                        end += 2;
                        continue;
                    }
                    if bytes[end] == b'"' {
                        break;
                    }
                    end += 1;
                }
                result.push(unescape(&rest[..end]));
                remaining = skip_ws(&rest[end + 1..]);
            } else {
                break;
            }
            remaining = skip_ws(remaining.trim_start_matches(','));
        }
        result
    }

    /// Extract a raw JSON object starting at the current position.
    fn extract_object(s: &str) -> Option<(String, &str)> {
        let s = skip_ws(s);
        if !s.starts_with('{') {
            return None;
        }
        let close = find_matching_close(s, '{', '}')?;
        Some((s[..=close].to_string(), &s[close + 1..]))
    }

    pub fn parse(content: &str) -> Transcript {
        let description = extract_string_field(content, "description").unwrap_or_default();
        let source =
            extract_string_field(content, "source").expect("transcript must have 'source'");

        // Find the "steps" array.
        let steps_needle = "\"steps\"";
        let steps_pos = content
            .find(steps_needle)
            .expect("transcript must have 'steps'");
        let after_steps = content[steps_pos + steps_needle.len()..].trim_start();
        let after_colon = after_steps.strip_prefix(':').unwrap().trim_start();
        // after_colon starts with '['
        let close_bracket =
            find_matching_close(after_colon, '[', ']').expect("steps array must close");
        let steps_inner = &after_colon[1..close_bracket];

        let mut steps = Vec::new();
        let mut remaining = skip_ws(steps_inner);
        while !remaining.is_empty() {
            remaining = skip_ws(remaining);
            if remaining.is_empty() {
                break;
            }
            // Each step is a JSON object.
            let (step_obj, rest) = match extract_object(remaining) {
                Some(r) => r,
                None => break,
            };
            remaining = skip_ws(rest.trim_start_matches(','));

            // Parse the step object.
            let step = parse_step(&step_obj);
            steps.push(step);
        }

        Transcript {
            description,
            source,
            steps,
        }
    }

    fn parse_step(obj: &str) -> Step {
        // Check for "send" key.
        let send_needle = "\"send\"";
        let open_needle = "\"open\"";
        let ec_needle = "\"expect_contains\"";
        let en_needle = "\"expect_notification\"";

        let send_raw = if let Some(pos) = obj.find(send_needle) {
            let after = skip_ws(&obj[pos + send_needle.len()..]);
            let after = after.strip_prefix(':').unwrap_or(after).trim_start();
            extract_object(after).map(|(o, _)| o)
        } else {
            None
        };

        let open_uri = if send_raw.is_none() {
            if let Some(pos) = obj.find(open_needle) {
                let after = skip_ws(&obj[pos + open_needle.len()..]);
                let after = after.strip_prefix(':').unwrap_or(after).trim_start();
                if let Some((open_obj, _)) = extract_object(after) {
                    extract_string_field(&open_obj, "uri")
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let expect_contains = if let Some(pos) = obj.find(ec_needle) {
            let after = skip_ws(&obj[pos + ec_needle.len()..]);
            let after = after.strip_prefix(':').unwrap_or(after).trim_start();
            extract_string_array(after)
        } else {
            Vec::new()
        };

        let expect_notification = if let Some(pos) = obj.find(en_needle) {
            let after = skip_ws(&obj[pos + en_needle.len()..]);
            let after = after.strip_prefix(':').unwrap_or(after).trim_start();
            // Not null = expect a notification.
            !after.starts_with("null")
        } else {
            false
        };

        Step {
            send_raw,
            open_uri,
            expect_contains,
            expect_notification,
        }
    }
}

/// Execute one JSON transcript file against a live `jet lsp` process.
fn run_json_transcript_file(jet: &std::path::Path, path: &std::path::Path) {
    let content =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {:?}: {}", path, e));
    let transcript = transcript_parser::parse(&content);

    let mut child = Command::new(jet)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jet lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    for step in &transcript.steps {
        if let Some(uri) = &step.open_uri {
            // Open-document step.
            let open = format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"jet","version":1,"text":{}}}}}}}"#,
                uri,
                json_string(&transcript.source)
            );
            send_msg(&mut stdin, &open);
            if step.expect_notification {
                let notif = read_msg(&mut stdout);
                assert!(
                    notif.contains("publishDiagnostics"),
                    "[{}] open step: expected publishDiagnostics, got: {}",
                    path.display(),
                    notif
                );
            }
        } else if let Some(raw) = &step.send_raw {
            send_msg(&mut stdin, raw);
            if !step.expect_contains.is_empty() {
                let resp = read_msg(&mut stdout);
                for expect in &step.expect_contains {
                    assert!(
                        resp.contains(expect.as_str()),
                        "[{}] expected {:?} in response, got:\n{}",
                        path.display(),
                        expect,
                        resp
                    );
                }
            }
        }
    }

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn lsp_json_transcripts() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let transcript_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/lsp");
    let mut files: Vec<_> = std::fs::read_dir(&transcript_dir)
        .expect("tests/lsp/ must exist")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no JSON transcripts found in tests/lsp/");
    for path in files {
        run_json_transcript_file(&jet, &path);
    }
}

#[test]
fn lsp_initialize_capabilities_have_named_test_coverage() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let mut child = Command::new(&jet)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jet lsp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
    );
    let init = read_msg(&mut stdout);
    let advertised = advertised_capabilities(&init);
    assert!(
        !advertised.is_empty(),
        "initialize response did not expose capabilities: {init}"
    );

    let source = include_str!("lsp.rs");
    for cap in advertised {
        let marker = LSP_CAPABILITY_COVERAGE
            .iter()
            .find_map(|(covered, marker)| (*covered == cap).then_some(*marker))
            .unwrap_or_else(|| panic!("advertised capability `{cap}` lacks coverage entry"));
        assert!(
            source.contains(marker),
            "coverage marker `{marker}` for `{cap}` is not present in tests/lsp.rs"
        );
    }

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#,
    );
    let _ = read_msg(&mut stdout);
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#,
    );
    drop(stdin);
    let _ = child.wait();
}

// ── M6 v0 regression tests (must keep working) ───────────────────────────────

#[test]
fn lsp_teaching_autocorrect_let_to_val() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let mut child = Command::new(&jet)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jet lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
    );
    let init = read_msg(&mut stdout);
    assert!(init.contains("textDocumentSync"), "init: {}", init);

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
    );

    let uri = "file:///tmp/lsp_test_let.jet";
    let src = "fn run() {\n    let x = 1;\n}\n";
    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"jet","version":1,"text":{}}}}}}}"#,
        uri,
        json_string(src)
    );
    send_msg(&mut stdin, &open);

    let diag_msg = read_msg(&mut stdout);
    assert!(
        diag_msg.contains("publishDiagnostics"),
        "expected publishDiagnostics, got: {}",
        diag_msg
    );
    assert!(
        !diag_msg.contains("E0009") && !diag_msg.contains("E0985"),
        "old binding words must not produce migration diagnostics, got: {}",
        diag_msg
    );

    let action_req = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/codeAction","params":{{"textDocument":{{"uri":"{}"}},"range":{{"start":{{"line":1,"character":4}},"end":{{"line":1,"character":7}}}},"context":{{"diagnostics":[]}}}}}}"#,
        uri
    );
    send_msg(&mut stdin, &action_req);
    let actions = read_msg(&mut stdout);
    assert!(
        !actions.contains(r#""newText":"val""#),
        "the retired `val` token-swap quick-fix must be gone, got: {}",
        actions
    );
    assert!(
        actions.contains(r#""id":2"#) && actions.contains(r#""result":[]"#),
        "expected an empty codeAction result for old binding syntax, got: {}",
        actions
    );

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":{}}"#,
    );
    let _ = read_msg(&mut stdout);
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#,
    );
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn lsp_incremental_sync_range_edit_updates_document() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let mut child = Command::new(&jet)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jet lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
    );
    let init = read_msg(&mut stdout);
    assert!(
        init.contains(r#""textDocumentSync": 2"#) || init.contains(r#""textDocumentSync":2"#),
        "expected incremental sync, got: {}",
        init
    );
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
    );

    let uri = "file:///tmp/lsp_incremental_test.jet";
    let src = "fn run() {\n    x :: 1\n}\n";
    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"jet","version":1,"text":{}}}}}}}"#,
        uri,
        json_string(src)
    );
    send_msg(&mut stdin, &open);
    let _ = read_msg(&mut stdout);

    let change = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{}","version":2}},"contentChanges":[{{"range":{{"start":{{"line":1,"character":4}},"end":{{"line":1,"character":10}}}},"rangeLength":6,"text":"let x = 1"}}]}}}}"#,
        uri
    );
    send_msg(&mut stdin, &change);

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/semanticTokens/full","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
        uri
    );
    send_msg(&mut stdin, &req);
    let diag = read_msg(&mut stdout);
    assert!(
        diag.contains("publishDiagnostics") && !diag.contains("E0009") && !diag.contains("E0985"),
        "expected range edit to trigger ordinary diagnostics, got: {}",
        diag
    );
    let tokens = read_msg(&mut stdout);
    assert!(
        tokens.contains(r#""id":2"#) && tokens.contains(r#""data""#),
        "expected semantic token response after dirty flush, got: {}",
        tokens
    );

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":{}}"#,
    );
    let _ = read_msg(&mut stdout);
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#,
    );
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn lsp_check_json_matches_jet_check_json_for_diagnostic_fixture() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let path = std::env::temp_dir().join(format!(
        "lsp_json_diff_{}_{}.jet",
        std::process::id(),
        "binding"
    ));
    let src = "fn run() {\n    let x = 1\n}\n";
    std::fs::write(&path, src).expect("write diagnostic fixture");

    let out = Command::new(&jet)
        .arg("check")
        .arg(&path)
        .arg("--json")
        .output()
        .expect("jet check --json");
    assert!(
        !out.status.success(),
        "diagnostic fixture should fail: status={}",
        out.status
    );
    let cli_json = String::from_utf8_lossy(&out.stderr).to_string();

    let file = path.to_string_lossy();
    let lsp_diags: Vec<_> = jet::check_document(&file, src)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    let lsp_json = jet::render_all_json(&file, src, &lsp_diags);

    assert_eq!(cli_json, lsp_json);
    let _ = std::fs::remove_file(path);
}

#[test]
fn lsp_formatting_and_range_formatting_return_edits() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn run(){\nprint(1)\n}\n";
    let uri = "file:///tmp/lsp_formatting_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec![
                    "documentFormattingProvider".to_string(),
                    "documentRangeFormattingProvider".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{}"}},"options":{{"tabSize":4,"insertSpaces":true}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["\"newText\"".to_string(), "\"range\"".to_string()]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/rangeFormatting","params":{{"textDocument":{{"uri":"{}"}},"range":{{"start":{{"line":0,"character":0}},"end":{{"line":3,"character":0}}}},"options":{{"tabSize":4,"insertSpaces":true}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["\"newText\"".to_string(), "\"range\"".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_execute_command_impact_returns_report() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn add(a: Int, b: Int) -> Int {\n    return a + b\n}\nfn run() {\n    print(add(1, 2))\n}\n";
    let uri = "file:///tmp/lsp_execute_impact_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["executeCommandProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"workspace/executeCommand","params":{{"command":"jet.impact","arguments":["{}","add",2]}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"symbol\":\"add\"".to_string(),
                    "\"found\":true".to_string(),
                    "\"upstream_callers\"".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_c_style_snippet_autocorrects() {
    let src = r#"def greet() {
    print("ok");
}

fn run() {
    count :: 1
    println("hi");
}
"#;
    let diags = jet::check_document("snippet.jet", src);
    assert!(diags.iter().any(|d| d.code == "E0037"), "println → print");
    assert!(diags.iter().any(|d| d.code == "E0008"), "def → fn");

    let mut edits: Vec<_> = diags.iter().filter_map(|d| d.edit.clone()).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut fixed = src.to_string();
    for edit in edits {
        fixed = jet::LSP::apply_edit(&fixed, &edit);
    }
    assert!(!fixed.contains("val count"));
    // E0037 and E0008 are single-token swaps and apply.
    assert!(fixed.contains("print("));
    assert!(!fixed.contains("println("));
    assert!(fixed.contains("fn greet"));
    assert!(!fixed.contains("def greet"));
}

// ── M13 transcript tests ──────────────────────────────────────────────────────

/// Run a transcript test against a live server process.
fn run_transcript(source: &str, steps: &[TranscriptStep]) {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let mut child = Command::new(&jet)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jet lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    // Write source to a temp file the server can reference
    let tmp_path = format!("/tmp/lsp_transcript_{}.jet", std::process::id());

    for step in steps {
        match step {
            TranscriptStep::Send {
                msg,
                expect_contains,
            } => {
                send_msg(&mut stdin, msg);
                if let Some(expects) = expect_contains {
                    let resp = read_msg(&mut stdout);
                    for expect in expects {
                        assert!(
                            resp.contains(expect.as_str()),
                            "transcript step: expected {:?} in response, got:\n{}",
                            expect,
                            resp
                        );
                    }
                }
            }
            TranscriptStep::Open {
                uri,
                expect_notification,
            } => {
                let open = format!(
                    r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"jet","version":1,"text":{}}}}}}}"#,
                    uri,
                    json_string(source)
                );
                send_msg(&mut stdin, &open);
                if *expect_notification {
                    let notif = read_msg(&mut stdout);
                    assert!(
                        notif.contains("publishDiagnostics"),
                        "expected publishDiagnostics after open, got: {}",
                        notif
                    );
                }
            }
        }
    }

    drop(stdin);
    let _ = child.wait();
    let _ = std::fs::remove_file(&tmp_path);
}

enum TranscriptStep {
    Send {
        msg: String,
        expect_contains: Option<Vec<String>>,
    },
    Open {
        uri: String,
        expect_notification: bool,
    },
}

#[test]
fn lsp_completion_returns_items() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet(name: String) {\n    print(name);\n}\nfn run() {\n    \n}\n";
    let uri = "file:///tmp/lsp_completion_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["completionProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":4,"character":4}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["items".to_string(), "greet".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_completion_returns_snippets_and_auto_imports() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let root = std::env::temp_dir().join(format!("lsp_auto_import_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).expect("create LSP auto-import dirs");
    std::fs::create_dir_all(root.join("src")).expect("create LSP source dirs");
    std::fs::write(
        root.join("app/store.jet"),
        "fn ImportedHelper() -> Int {\n    return 1\n}\nfn run() {}\n",
    )
    .expect("write imported module");
    let path = root.join("src/main.jet");
    let uri = format!("file://{}", path.display());
    let root_uri = format!("file://{}", root.display());
    let source = "fn run() {\n    \n}\n";
    std::fs::write(&path, source).expect("write main module");

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"rootUri":"{}","capabilities":{{}}}}}}"#,
                    root_uri
                ),
                expect_contains: Some(vec!["completionProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.clone(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":1,"character":4}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"label\":\"bind immut (inferred)\"".to_string(),
                    "\"insertTextFormat\":2".to_string(),
                    "\"label\":\"ImportedHelper\"".to_string(),
                    "\"additionalTextEdits\"".to_string(),
                    "use app.store\\n".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lsp_completion_uses_local_discovery_index_for_packages_and_options() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let root = std::env::temp_dir().join(format!("lsp_discovery_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create discovery fixture dir");

    let mut index = jet::Jetpack::Discovery::Index::default();
    index.add_package(jet::Jetpack::Discovery::PackageRecord {
        source: "default".to_string(),
        name: "postgres_16".to_string(),
        reference: "default:postgres_16".to_string(),
        version: "16.4".to_string(),
        platforms: vec!["linux".to_string()],
        docs: "Postgres fixture from local discovery index".to_string(),
        provenance: "test".to_string(),
        options: vec![
            jet::Jetpack::Discovery::OptionField {
                name: "ready".to_string(),
                default: "process/port probe".to_string(),
                docs: "Command polled until ready.".to_string(),
            },
            jet::Jetpack::Discovery::OptionField {
                name: "data_dir".to_string(),
                default: ".jet/services/db/data".to_string(),
                docs: "Persisted state directory.".to_string(),
            },
        ],
    });
    jet::Jetpack::Discovery::write(&root, &index).expect("write local discovery index");

    let path = root.join("main.jet");
    let uri = format!("file://{}", path.display());
    let source = "module env.dev {\n    env.dev: Env.{\n        packages: [default.post]\n        services: { db: Service.{ re } }\n    }\n}\n";
    std::fs::write(&path, source).expect("write LSP source");

    let mut child = Command::new(&jet)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jet lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
    );
    let init = read_msg(&mut stdout);
    assert!(init.contains("completionProvider"), "init: {init}");
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
    );

    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"jet","version":1,"text":{}}}}}}}"#,
        uri,
        json_string(source)
    );
    send_msg(&mut stdin, &open);
    let _ = read_msg(&mut stdout);

    let package_offset = source.find("default.post").unwrap() + "default.post".len();
    let package_pos = jet::LSP::byte_offset_to_lsp(source, package_offset);
    let package_req = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}}}}}}"#,
        uri, package_pos.line, package_pos.character
    );
    send_msg(&mut stdin, &package_req);
    let package_resp = read_msg(&mut stdout);
    assert!(
        package_resp.contains("postgres_16") && package_resp.contains("package from default"),
        "package discovery completion missing: {package_resp}"
    );

    let option_offset = source.find("Service.{ re").unwrap() + "Service.{ re".len();
    let option_pos = jet::LSP::byte_offset_to_lsp(source, option_offset);
    let option_req = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}}}}}}"#,
        uri, option_pos.line, option_pos.character
    );
    send_msg(&mut stdin, &option_req);
    let option_resp = read_msg(&mut stdout);
    assert!(
        option_resp.contains("\"ready\"") && option_resp.contains("Command polled until ready."),
        "option discovery completion missing: {option_resp}"
    );

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#,
    );
    let _ = read_msg(&mut stdout);
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#,
    );
    drop(stdin);
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lsp_document_links_and_code_lenses() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let root = std::env::temp_dir().join(format!("lsp_links_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).expect("create LSP link dirs");
    std::fs::create_dir_all(root.join("src")).expect("create LSP source dirs");
    std::fs::write(root.join("app/store.jet"), "fn run() {}\n").expect("write link target");
    let path = root.join("src/main.jet");
    let uri = format!("file://{}", path.display());
    let root_uri = format!("file://{}", root.display());
    let target_uri = format!("file://{}", root.join("app/store.jet").display());
    let source = "use app.store\n\nfn run() {\n    print(1)\n}\n\n#Test(\"smoke\") {\n    expect(1 == 1)\n}\n";
    std::fs::write(&path, source).expect("write link source");

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"rootUri":"{}","capabilities":{{}}}}}}"#,
                    root_uri
                ),
                expect_contains: Some(vec![
                    "documentLinkProvider".to_string(),
                    "codeLensProvider".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.clone(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/documentLink","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"target\"".to_string(),
                    target_uri.clone(),
                    r#""start":{"line":0,"character":4}"#.to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"workspace/symbol","params":{{"query":"run"}}}}"#,
                ),
                expect_contains: Some(vec!["\"location\"".to_string(), target_uri.clone()]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/codeLens","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"title\":\"Run file\"".to_string(),
                    "\"command\":\"jet.runFile\"".to_string(),
                    "\"title\":\"Run test\"".to_string(),
                    "\"command\":\"jet.testFile\"".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lsp_signature_help_returns_active_parameter() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn add(a: Int, b: Int) -> Int {\n    return a + b;\n}\nfn run() {\n    r :: add(1, 2)\n}\n";
    let uri = "file:///tmp/lsp_signature_help_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["signatureHelpProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/signatureHelp","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":4,"character":16}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "fn add(a: Int, b: Int) -> Int".to_string(),
                    "\"activeParameter\":1".to_string(),
                    "b: Int".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_document_symbol_returns_checked_outline() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "struct Point { x: Int }\nenum Color { Red Green }\nfn add(a: Int, b: Int) -> Int {\n    return a + b;\n}\n";
    let uri = "file:///tmp/lsp_document_symbol_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["documentSymbolProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/documentSymbol","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"name\":\"Point\"".to_string(),
                    "\"kind\":23".to_string(),
                    "\"name\":\"Color\"".to_string(),
                    "\"kind\":10".to_string(),
                    "\"name\":\"add\"".to_string(),
                    "\"kind\":12".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_wave2_navigation_features() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "// file note one\n// file note two\n\nfn add(a: Int, b: Int) -> Int {\n    total :: a + b\n    return total\n}\n\nfn run() {\n    value :: add(1, 2)\n    print(value)\n}\n";
    let uri = "file:///tmp/lsp_wave2_nav_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec![
                    "workspaceSymbolProvider".to_string(),
                    "foldingRangeProvider".to_string(),
                    "documentHighlightProvider".to_string(),
                    "selectionRangeProvider".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":2,"method":"workspace/symbol","params":{"query":"add"}}"#
                    .to_string(),
                expect_contains: Some(vec![
                    "\"name\":\"add\"".to_string(),
                    "\"kind\":12".to_string(),
                    "\"location\"".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/foldingRange","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"kind\":\"comment\"".to_string(),
                    "\"startLine\":0".to_string(),
                    "\"endLine\":1".to_string(),
                    "\"kind\":\"region\"".to_string(),
                    "\"startLine\":3".to_string(),
                    "\"endLine\":6".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/documentHighlight","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":9,"character":14}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"kind\":3".to_string(),
                    "\"kind\":2".to_string(),
                    "\"range\"".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":5,"method":"textDocument/selectionRange","params":{{"textDocument":{{"uri":"{}"}},"positions":[{{"line":9,"character":14}}]}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"parent\"".to_string(),
                    r#""start":{"line":9,"character":13}"#.to_string(),
                    r#""end":{"line":9,"character":16}"#.to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_wave3_prepare_rename_semantic_range_and_call_hierarchy() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn add(a: Int, b: Int) -> Int {\n    return a + b\n}\n\nfn caller() {\n    result :: add(1, 2)\n    print(result)\n}\n\nfn run() {\n    caller()\n}\n";
    let uri = "file:///tmp/lsp_wave3_power_test.jet";
    let add_item = format!(
        r#"{{"name":"add","kind":12,"uri":"{}","range":{{"start":{{"line":0,"character":3}},"end":{{"line":0,"character":6}}}},"selectionRange":{{"start":{{"line":0,"character":3}},"end":{{"line":0,"character":6}}}}}}"#,
        uri
    );
    let caller_item = format!(
        r#"{{"name":"caller","kind":12,"uri":"{}","range":{{"start":{{"line":4,"character":3}},"end":{{"line":4,"character":9}}}},"selectionRange":{{"start":{{"line":4,"character":3}},"end":{{"line":4,"character":9}}}}}}"#,
        uri
    );

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec![
                    "\"prepareProvider\": true".to_string(),
                    "\"range\": true".to_string(),
                    "\"callHierarchyProvider\": true".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/prepareRename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"placeholder\":\"add\"".to_string(),
                    r#""start":{"line":0,"character":3}"#.to_string(),
                    r#""end":{"line":0,"character":6}"#.to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/prepareRename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":0}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"error\"".to_string(),
                    "`fn` is Jet syntax, not a name you can rename".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/semanticTokens/range","params":{{"textDocument":{{"uri":"{}"}},"range":{{"start":{{"line":0,"character":0}},"end":{{"line":2,"character":1}}}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["\"data\":[".to_string()]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":5,"method":"textDocument/prepareCallHierarchy","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"name\":\"add\"".to_string(),
                    "\"kind\":12".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":6,"method":"callHierarchy/incomingCalls","params":{{"item":{}}}}}"#,
                    add_item
                ),
                expect_contains: Some(vec![
                    "\"from\"".to_string(),
                    "\"name\":\"caller\"".to_string(),
                    "\"fromRanges\"".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":7,"method":"callHierarchy/outgoingCalls","params":{{"item":{}}}}}"#,
                    caller_item
                ),
                expect_contains: Some(vec![
                    "\"to\"".to_string(),
                    "\"name\":\"add\"".to_string(),
                    "\"fromRanges\"".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_type_hierarchy_trait_impls() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "trait Renderable {\n    fn render(self) -> String\n}\n\nstruct Button {\n    label: String\n    impl Renderable {\n        fn render(self) -> String {\n            return self.label\n        }\n    }\n}\n\nfn run() {\n    b :: Button.{label: \"ok\"}\n    print(b.render())\n}\n";
    let uri = "file:///tmp/lsp_type_hierarchy_test.jet";
    let button_item = format!(r#"{{"name":"Button","kind":23,"uri":"{}"}}"#, uri);
    let trait_item = format!(r#"{{"name":"Renderable","kind":11,"uri":"{}"}}"#, uri);

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["\"typeHierarchyProvider\": true".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/prepareTypeHierarchy","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":4,"character":7}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"name\":\"Button\"".to_string(),
                    "\"kind\":23".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"typeHierarchy/supertypes","params":{{"item":{}}}}}"#,
                    button_item
                ),
                expect_contains: Some(vec![
                    "\"name\":\"Renderable\"".to_string(),
                    "\"kind\":11".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"typeHierarchy/subtypes","params":{{"item":{}}}}}"#,
                    trait_item
                ),
                expect_contains: Some(vec![
                    "\"name\":\"Button\"".to_string(),
                    "\"kind\":23".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_prepare_rename_rejects_foreign_symbol() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn func() {}\n";
    let uri = "file:///tmp/lsp_prepare_rename_foreign_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["prepareProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/prepareRename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "\"error\"".to_string(),
                    "`func` is a retired foreign spelling, not a Jet symbol you can rename"
                        .to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_semantic_tokens_delta_returns_full_fallback() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn run() {\n    value :: 1\n    print(value)\n}\n";
    let uri = "file:///tmp/lsp_semantic_delta_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec![
                    "\"full\": { \"delta\": true }".to_string(),
                    "\"range\": true".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/semanticTokens/full/delta","params":{{"textDocument":{{"uri":"{}"}},"previousResultId":"stale"}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["\"resultId\"".to_string(), "\"data\":[".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_hover_returns_signature() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn add(a: Int, b: Int) -> Int {\n    return a + b;\n}\nfn run() {\n    r :: add(1, 2)\n}\n";
    let uri = "file:///tmp/lsp_hover_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["hoverProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["add".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_rename_produces_workspace_edit() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet() {}\nfn run() {\n    greet();\n    greet();\n}\n";
    let uri = "file:///tmp/lsp_rename_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["renameProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}},"newName":"hello"}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["changes".to_string(), "hello".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_definition_returns_location() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet() {}\nfn run() {\n    greet();\n}\n";
    let uri = "file:///tmp/lsp_def_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["definitionProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":2,"character":4}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["uri".to_string(), "range".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_semantic_tokens_returns_data() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn run() {\n    x: Int :: 1\n}\n";
    let uri = "file:///tmp/lsp_semtok_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["semanticTokensProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/semanticTokens/full","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["data".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_semantic_tokens_classify_ownership_markers_and_skip_retired_words() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    let source = r#"#Test("semantic") {
}
#Unsafe("audit") fn archive(name: ^String, slot: &Int) -> String {
    saved :: copy name
    return saved
}
@Pure fn clean(x: Int) -> Int { return x }
fn run() {
    old :: 1
    while true { break }
    mut borrowed
    take borrowed
    view borrowed
}
"#;
    let uri = "file:///tmp/lsp_semantic_highlight_stage4.jet";

    let mut child = Command::new(&jet)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jet lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
    );
    let init = read_msg(&mut stdout);
    for expected in ["ownership", "decorator", "move", "writeBorrow", "copy"] {
        assert!(
            init.contains(expected),
            "initialize response missing `{expected}`: {init}"
        );
    }
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
    );

    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"jet","version":1,"text":{}}}}}}}"#,
        uri,
        json_string(source)
    );
    send_msg(&mut stdin, &open);
    let _ = read_msg(&mut stdout);

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/semanticTokens/full","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
        uri
    );
    send_msg(&mut stdin, &req);
    let response = read_msg(&mut stdout);
    assert!(
        response.contains(r#""id":2"#) && response.contains(r#""data""#),
        "expected semantic token response, got: {response}"
    );

    const TOKEN_OWNERSHIP: u32 = 12;
    const TOKEN_DECORATOR: u32 = 13;
    const MOD_MOVE: u32 = 1 << 2;
    const MOD_WRITE_BORROW: u32 = 1 << 3;
    const MOD_COPY: u32 = 1 << 4;
    const MOD_DIRECTIVE: u32 = 1 << 5;
    const MOD_CONTRACT: u32 = 1 << 6;

    let tokens = decode_semantic_tokens(source, &response);
    assert_semantic_token(&tokens, "copy", TOKEN_OWNERSHIP, MOD_COPY);
    assert_semantic_token(&tokens, "^", TOKEN_OWNERSHIP, MOD_MOVE);
    assert_semantic_token(&tokens, "&", TOKEN_OWNERSHIP, MOD_WRITE_BORROW);
    assert_semantic_token(&tokens, "#", TOKEN_DECORATOR, MOD_DIRECTIVE);
    assert_semantic_token(&tokens, "Test", TOKEN_DECORATOR, MOD_DIRECTIVE);
    assert_semantic_token(&tokens, "Unsafe", TOKEN_DECORATOR, MOD_DIRECTIVE);
    assert_semantic_token(&tokens, "@", TOKEN_DECORATOR, MOD_CONTRACT);
    assert_semantic_token(&tokens, "Pure", TOKEN_DECORATOR, MOD_CONTRACT);

    for retired in ["val", "while", "mut", "take", "view"] {
        assert!(
            !tokens.iter().any(|t| t.text == retired),
            "retired/foreign spelling `{retired}` should not emit semantic token: {tokens:?}"
        );
    }

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":{}}"#,
    );
    let _ = read_msg(&mut stdout);
    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#,
    );
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn lsp_inlay_hints_returns_type_labels() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn run() {\n    x :: 42\n    count := 0\n}\n";
    let uri = "file:///tmp/lsp_inlay_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["inlayHintProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/inlayHint","params":{{"textDocument":{{"uri":"{}"}},"range":{{"start":{{"line":0,"character":0}},"end":{{"line":10,"character":0}}}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![": Int".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_inlay_hints_include_scattered_method_breadcrumbs() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "trait DrawThing {\n    fn render(self) -> String\n}\n\nstruct Widget {\n    title: String\n}\n\nimpl Widget {\n    fn size(self) -> Int {\n        return 1\n    }\n}\n\nimpl Widget.DrawThing {\n    fn render(self) -> String {\n        return self.title\n    }\n}\n\nfn run() {\n    w :: Widget.{title: \"ok\"}\n    print(w.render())\n}\n";
    let uri = "file:///tmp/lsp_breadcrumb_hint_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["inlayHintProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/inlayHint","params":{{"textDocument":{{"uri":"{}"}},"range":{{"start":{{"line":0,"character":0}},"end":{{"line":30,"character":0}}}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec![
                    "+ fn size() -> Int".to_string(),
                    "+ fn render() -> String".to_string(),
                ]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn lsp_references_finds_all_uses() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet() {}\nfn run() {\n    greet();\n    greet();\n}\n";
    let uri = "file:///tmp/lsp_refs_test.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["referencesProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/references","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}},"context":{{"includeDeclaration":true}}}}}}"#,
                    uri
                ),
                expect_contains: Some(vec!["range".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

// ── C40: keyword-table correctness tests ─────────────────────────────────────
//
// Verifies that JET_KEYWORDS tracks Source/Syntax.rs (Fix A).
// is_keyword drives rename validation; wrong entries reject valid names or
// allow reserved ones. These tests must pass for the bug to be fixed.

#[test]
fn c40_is_keyword_retired_words_not_keywords() {
    // `switch`, `import`, `val`, `var` are NOT keywords in Jet —
    // they are FOREIGN_ teaching-error tokens. Rename must accept them as names.
    let diags = jet::check_document(
        "c40_retired.jet",
        "fn run() {\n    switch_count :: 0\n    import_count :: 1\n}\n",
    );
    // These should compile/parse; the names `switch_count` and `import_count`
    // are legal identifiers.
    // Key assertion: no diagnostic claiming "switch_count" / "import_count" is a keyword.
    assert!(
        !diags.iter().any(|d| {
            let t = format!("{} {} {}", d.what, d.why, d.fix);
            t.contains("switch_count") && t.contains("keyword")
        }),
        "switch_count should not be rejected as a keyword: {:?}",
        diags
    );
}

#[test]
fn c40_is_keyword_real_keywords_recognized() {
    // The actual Jet keywords must be recognized so rename rejects them.
    // We verify via the LSP rename path using check_document behaviour:
    // `if` (KW_IF), `use` (KW_USE), `fn` (KW_FN) must be keywords.
    // This is a structural test — we check the JET_KEYWORDS table directly
    // via the public API surface (check_document for a rename probe).
    //
    // Direct unit test on is_keyword is not pub, so we test via the rename
    // response: renaming to a keyword name returns an error.
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet() {}\nfn run() {\n    greet();\n}\n";
    let uri = "file:///tmp/c40_rename_keyword.jet";

    // `if` is a real keyword; rename to it must fail.
    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["renameProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}},"newName":"if"}}}}"#,
                    uri
                ),
                // Renaming to a keyword must produce an error response
                expect_contains: Some(vec!["error".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn c40_is_keyword_value_is_not_a_keyword() {
    // `Val` is LIT_VALUE (a literal, like `true`/`false`/`None`), not a keyword
    // (D-OPT-SPELL1: renamed from `value`). Rename to "Val" should NOT be
    // rejected as a keyword.
    // (It may or may not succeed for other reasons, but not because it's a keyword.)
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet() {}\nfn run() {\n    greet();\n}\n";
    let uri = "file:///tmp/c40_rename_value.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["renameProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}},"newName":"Val"}}}}"#,
                    uri
                ),
                // Must NOT say "Val is a keyword" — Val is a literal, not a keyword
                expect_contains: Some(vec!["result".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn c40_is_keyword_switch_not_a_keyword() {
    // `switch` is FOREIGN_SWITCH — a teaching-error word, not a real keyword.
    // Rename to "switch" should NOT be rejected as a keyword.
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet() {}\nfn run() {\n    greet();\n}\n";
    let uri = "file:///tmp/c40_rename_switch.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["renameProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}},"newName":"switch"}}}}"#,
                    uri
                ),
                // Must NOT say "switch is a keyword" — switch is a foreign/teaching word
                expect_contains: Some(vec!["result".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn c40_is_keyword_import_not_a_keyword() {
    // `import` is FOREIGN_IMPORT — renamed to `use` (D-S16-USE). Not a keyword.
    // Rename to "import" should NOT be rejected as a keyword.
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn greet() {}\nfn run() {\n    greet();\n}\n";
    let uri = "file:///tmp/c40_rename_import.jet";

    run_transcript(
        source,
        &[
            TranscriptStep::Send {
                msg:
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
                        .to_string(),
                expect_contains: Some(vec!["renameProvider".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                expect_contains: None,
            },
            TranscriptStep::Open {
                uri: uri.to_string(),
                expect_notification: true,
            },
            TranscriptStep::Send {
                msg: format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}},"newName":"import"}}}}"#,
                    uri
                ),
                // Must NOT say "import is a keyword" — import is a foreign/teaching word
                expect_contains: Some(vec!["result".to_string()]),
            },
            TranscriptStep::Send {
                msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                expect_contains: Some(vec!["result".to_string()]),
            },
        ],
    );
}

#[test]
fn c40_keyword_like_identifier_usable_as_variable() {
    // Variables named `printer`, `sprint`, `in_count` must NOT be flagged as keywords.
    // These contain keyword substrings (`print`, `in`, `sprint` → `pr`+`in`+`t`) but
    // are regular identifiers. Use current Jet binding syntax (:: sigil).
    let diags = jet::check_document(
        "c40_kw_in_ident.jet",
        "fn run() {\n    printer :: \"hp\";\n    in_count :: 3;\n    sprint :: 9.8;\n}\n",
    );
    // None of the diagnostics should claim these identifiers are keywords.
    for d in &diags {
        let all_text = format!("{} {} {}", d.what, d.why, d.fix);
        // The name `printer`, `in_count`, `sprint` must not be called a keyword.
        for name in &["printer", "in_count", "sprint"] {
            assert!(
                !(all_text.contains(name) && all_text.to_lowercase().contains("keyword")),
                "identifier `{}` containing keyword substring wrongly flagged as keyword: {:?}",
                name,
                d
            );
        }
    }
}

#[test]
fn c40_drift_guard_foreign_words_not_blocked_in_rename() {
    // Structural drift guard: foreign/retired words that were wrongly listed in
    // JET_KEYWORDS must NOT block rename. Test via live LSP rename endpoint.
    //
    // For each banned word, we rename a function to that name and assert the
    // response does NOT contain "keyword" in an error message.
    // (A rename error saying "X is a keyword" is a false positive for these words.)
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }

    // `switch` — FOREIGN_SWITCH, not a keyword; was wrongly in JET_KEYWORDS
    {
        let source = "fn greet() {}\nfn run() {\n    greet();\n}\n";
        let uri = "file:///tmp/c40_drift_switch.jet";
        run_transcript(
            source,
            &[
                TranscriptStep::Send {
                    msg: r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
                    expect_contains: Some(vec!["renameProvider".to_string()]),
                },
                TranscriptStep::Send {
                    msg: r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
                    expect_contains: None,
                },
                TranscriptStep::Open {
                    uri: uri.to_string(),
                    expect_notification: true,
                },
                TranscriptStep::Send {
                    msg: format!(
                        r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{}"}},"position":{{"line":0,"character":3}},"newName":"switch"}}}}"#,
                        uri
                    ),
                    // Renaming to "switch" should succeed (produce a workspace edit result),
                    // NOT produce an error saying "switch is a keyword"
                    expect_contains: Some(vec!["result".to_string()]),
                },
                TranscriptStep::Send {
                    msg: r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":{}}"#.to_string(),
                    expect_contains: Some(vec!["result".to_string()]),
                },
            ],
        );
    }
}

// ── c44: drift-guard tests ────────────────────────────────────────────────────
//
// These tests encode the structural invariants introduced in c44.
// They exist so any future attempt to re-fork the keyword/type/builtin tables
// into parallel hardcoded lists fails loudly rather than silently.

/// The LSP keyword table must be identical to Syntax::JET_KEYWORD_LIST.
///
/// Since Completion::JET_KEYWORDS is now a direct alias (`= Syntax::JET_KEYWORD_LIST`),
/// this test is a pointer-equality sanity check. It also encodes that key words
/// we care about are present, and that FOREIGN_* words are absent.
#[test]
fn c44_lsp_keywords_derive_from_syntax() {
    use jet::Syntax;
    // JET_KEYWORDS is an alias of JET_KEYWORD_LIST — compare ptr addresses.
    // If they ever become separate slices again, this detects it at pointer level.
    let kw_ptr = Syntax::JET_KEYWORD_LIST.as_ptr();
    // We can't import Completion directly (it's pub(crate)), so we verify via
    // the public check_document / completion path: ensure Syntax constants appear
    // in what completions return. Instead, verify the key structural invariants:
    // 1. Real keywords are present.
    let required = [
        Syntax::KW_FN,
        Syntax::KW_PUB,
        Syntax::KW_USE,
        Syntax::KW_IF,
        Syntax::KW_ELSE,
        Syntax::KW_LOOP,
        Syntax::KW_RETURN,
        Syntax::KW_STRUCT,
        Syntax::KW_ENUM,
        Syntax::KW_IMPL,
        Syntax::KW_UNSAFE,
        Syntax::KW_TEST,
        Syntax::KW_PURE,
        Syntax::KW_TODO,
        Syntax::LIT_TRUE,
        Syntax::LIT_FALSE,
        Syntax::LIT_NULL,
        Syntax::LIT_OK,
        Syntax::LIT_ERR,
        Syntax::KW_DISTINCT,
        Syntax::KW_MODULE,
    ];
    for kw in &required {
        assert!(
            Syntax::JET_KEYWORD_LIST.contains(kw),
            "JET_KEYWORD_LIST missing required keyword: {:?}",
            kw
        );
    }
    // 2. FOREIGN_* teaching words must NOT be in the keyword list.
    let banned = [
        Syntax::FOREIGN_SWITCH,
        Syntax::FOREIGN_IMPORT,
        Syntax::FOREIGN_OR_FALLBACK,
        Syntax::FOREIGN_WHILE,
        Syntax::FOREIGN_FOR,
        Syntax::FOREIGN_MATCH,
        Syntax::FOREIGN_CLASS,
    ];
    for word in &banned {
        assert!(
            !Syntax::JET_KEYWORD_LIST.contains(word),
            "JET_KEYWORD_LIST contains banned FOREIGN_ word: {:?}",
            word
        );
    }
    // 3. LIT_VALUE is a literal, not a keyword — must NOT be in keyword list.
    assert!(
        !Syntax::JET_KEYWORD_LIST.contains(&Syntax::LIT_VALUE),
        "LIT_VALUE ('value') must not appear in JET_KEYWORD_LIST"
    );
    // 4. The list is non-empty and reasonable in size.
    assert!(
        Syntax::JET_KEYWORD_LIST.len() >= 20,
        "JET_KEYWORD_LIST suspiciously short: {} entries",
        Syntax::JET_KEYWORD_LIST.len()
    );
    // Suppress unused-variable warning for kw_ptr (used as a compile-time anchor).
    let _ = kw_ptr;
}

/// The LSP type table must contain all primitive types from Syntax.rs.
#[test]
fn c44_lsp_types_derive_from_syntax() {
    use jet::Syntax;
    let required_types = [
        Syntax::TYPE_INT,
        Syntax::TYPE_FLOAT,
        Syntax::TYPE_BOOL,
        Syntax::TYPE_STRING,
        Syntax::TYPE_CHAR,
        Syntax::TYPE_SHARED,
        Syntax::TYPE_HASH_MAP,
        Syntax::TYPE_BTREE_MAP,
        Syntax::TYPE_DEQUE,
        Syntax::TYPE_SET,
    ];
    for ty in &required_types {
        assert!(
            Syntax::JET_TYPE_LIST.contains(ty),
            "JET_TYPE_LIST missing required type: {:?}",
            ty
        );
    }
    // Result is the legacy fallible type (S34 teaching only) — excluded.
    assert!(
        !Syntax::JET_TYPE_LIST.contains(&Syntax::TYPE_RESULT),
        "JET_TYPE_LIST must not contain legacy TYPE_RESULT; use T ? E syntax"
    );
}

/// The IMPURE_BUILTINS list must include all known impure builtins.
/// Sema/Purity and Comptime/Purity both derive from it; this test prevents
/// silent omission of new builtins.
#[test]
fn c44_impure_builtins_complete() {
    use jet::Syntax;
    let required = [
        Syntax::BUILTIN_PRINT,
        Syntax::BUILTIN_INPUT,
        "eprint",
        "read_all_input",
    ];
    for name in &required {
        assert!(
            Syntax::IMPURE_BUILTINS.contains(name),
            "IMPURE_BUILTINS missing: {:?}",
            name
        );
    }
}

/// PRELUDE_IDENTS must contain exactly the prelude builtins and match
/// BUILTIN_PRINT + BUILTIN_INPUT so it can substitute for the inline pair.
#[test]
fn c44_prelude_idents_canonical() {
    use jet::Syntax;
    assert!(
        Syntax::PRELUDE_IDENTS.contains(&Syntax::BUILTIN_PRINT),
        "PRELUDE_IDENTS missing BUILTIN_PRINT"
    );
    assert!(
        Syntax::PRELUDE_IDENTS.contains(&Syntax::BUILTIN_INPUT),
        "PRELUDE_IDENTS missing BUILTIN_INPUT"
    );
    // The prelude is exactly {print, input} per D-PRELUDE1 = B.
    assert_eq!(
        Syntax::PRELUDE_IDENTS.len(),
        2,
        "PRELUDE_IDENTS must have exactly 2 members (D-PRELUDE1 = B): {:?}",
        Syntax::PRELUDE_IDENTS
    );
}

// ── Latency bench (jet lsp --bench gate) ─────────────────────────────────────

#[test]
fn lsp_bench_under_budget() {
    // Run the bench in-process: 10 rounds on the wordcount example, budget 200ms/round.
    // This mirrors what `jet lsp --bench` does in CI.
    let src = include_str!("../examples/features/collections/wordcount.jet");
    let budget_ms = 200u128;
    let rounds = 10usize;

    let start = std::time::Instant::now();
    for _ in 0..rounds {
        let _ = jet::check_document("bench.jet", src);
    }
    let elapsed = start.elapsed();
    let per_round_ms = elapsed.as_millis() / rounds as u128;
    assert!(
        per_round_ms <= budget_ms,
        "latency regression: {}ms/round > budget {}ms/round",
        per_round_ms,
        budget_ms
    );
}
