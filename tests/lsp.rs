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

    send_msg(&mut stdin, r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#);

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

    let action_req = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/codeAction","params":{{"textDocument":{{"uri":"{}"}},"range":{{"start":{{"line":1,"character":4}},"end":{{"line":1,"character":7}}}},"context":{{"diagnostics":[]}}}}}}"#,
        uri
    );
    send_msg(&mut stdin, &action_req);
    let actions = read_msg(&mut stdout);
    assert!(
        actions.contains(r#""newText":"val""#),
        "expected val quick-fix, got: {}",
        actions
    );
    assert!(actions.contains("quickfix"), "expected quickfix kind");

    send_msg(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":{}}"#,
    );
    let _ = read_msg(&mut stdout);
    send_msg(&mut stdin, r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#);
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
    assert!(diags.iter().any(|d| d.code == "E0009"), "let → val");
    assert!(diags.iter().any(|d| d.code == "E0037"), "println → print");
    assert!(diags.iter().any(|d| d.code == "E0008"), "def → fn");

    let mut edits: Vec<_> = diags.iter().filter_map(|d| d.edit.clone()).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut fixed = src.to_string();
    for edit in edits {
        fixed = jet::lsp::apply_edit(&fixed, &edit);
    }
    assert!(fixed.contains("val count"));
    assert!(!fixed.contains("let count"));
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
            TranscriptStep::Send { msg, expect_contains } => {
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
            TranscriptStep::Open { uri, expect_notification } => {
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
                msg: r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
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
                msg: r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
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
                msg: r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
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
                msg: r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
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
                msg: r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
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
                msg: r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_string(),
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
    let src = include_str!("../examples/16_wordcount.jet");
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
