//! M6 phase 4: scripted LSP integration tests.

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
