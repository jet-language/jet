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
    let src = "fn main() {\n    let x = 1;\n}\n";
    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"jet","version":1,"text":{}}}}}}}"#,
        uri,
        json_string(src)
    );
    send_msg(&mut stdin, &open);

    let diag_msg = read_msg(&mut stdout);
    assert!(
        diag_msg.contains("E0009"),
        "expected E0009 diagnostic, got: {}",
        diag_msg
    );
    assert!(
        diag_msg.contains("publishDiagnostics"),
        "expected publishDiagnostics, got: {}",
        diag_msg
    );

    // D-BIND1: `let x = 1` migrates to `x :: 1`, which moves tokens — it is no
    // longer a single-keyword swap, so the E0009 teaching diagnostic carries no
    // trivial quick-fix edit (the codeAction result is empty). `jet fmt`
    // performs the migration.
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
        "expected an empty codeAction result for E0009 (no trivial edit), got: {}",
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
fn lsp_c_style_snippet_autocorrects() {
    let src = r#"def greet() {
    print("ok");
}

fn main() {
    let count = 1;
    println("hi");
}
"#;
    let diags = jet::check_document("snippet.jet", src);
    assert!(diags.iter().any(|d| d.code == "E0009"), "let → binding sigil");
    assert!(diags.iter().any(|d| d.code == "E0037"), "println → print");
    assert!(diags.iter().any(|d| d.code == "E0008"), "def → fn");

    let mut edits: Vec<_> = diags.iter().filter_map(|d| d.edit.clone()).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut fixed = src.to_string();
    for edit in edits {
        fixed = jet::lsp::apply_edit(&fixed, &edit);
    }
    // D-BIND1: E0009 (`let`) carries no token-swap edit — migrating to `count :: 1`
    // moves tokens, so `jet fmt` handles it and the LSP leaves `let` in place.
    assert!(fixed.contains("let count"));
    assert!(!fixed.contains("val count"));
    // E0037 and E0008 are still single-token swaps and apply.
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
    let source = "fn greet(name: String) {\n    print(name);\n}\nfn main() {\n    \n}\n";
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
fn lsp_hover_returns_signature() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn add(a: Int, b: Int) -> Int {\n    return a + b;\n}\nfn main() {\n    val r = add(1, 2);\n}\n";
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
    let source = "fn greet() {}\nfn main() {\n    greet();\n    greet();\n}\n";
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
    let source = "fn greet() {}\nfn main() {\n    greet();\n}\n";
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
    let source = "fn main() {\n    val x: Int = 1;\n}\n";
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
fn lsp_inlay_hints_returns_type_labels() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let source = "fn main() {\n    val x = 42;\n    val s = \"hello\";\n}\n";
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
                expect_contains: Some(vec!["Int".to_string()]),
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
    let source = "fn greet() {}\nfn main() {\n    greet();\n    greet();\n}\n";
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

// ── Latency bench (jet lsp --bench gate) ─────────────────────────────────────

#[test]
fn lsp_bench_under_budget() {
    // Run the bench in-process: 10 rounds on the wordcount example, budget 200ms/round.
    // This mirrors what `jet lsp --bench` does in CI.
    let src = include_str!("../examples/features/16_wordcount.jet");
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
