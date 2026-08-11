//! Card #456: production Jetpack must not use `unwrap()` or `expect(...)`.
//!
//! This scanner is deliberately lexical and std-only. It ignores Rust text
//! that cannot execute, removes definitely test-only items, and treats unknown
//! cfg predicates conservatively as production.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    Punct(char),
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Finding {
    path: String,
    line: usize,
    method: String,
    item: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Truth {
    Yes,
    No,
    Maybe,
}

fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }
        if let Some((end, newlines)) = raw_string_end(bytes, i) {
            line += newlines;
            i = end;
            continue;
        }
        let normal_prefix = if bytes[i..].starts_with(b"b\"")
            || bytes[i..].starts_with(b"c\"")
        {
            Some(2)
        } else if bytes[i] == b'"' {
            Some(1)
        } else {
            None
        };
        if let Some(prefix) = normal_prefix {
            i += prefix;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }
        if bytes[i] == b'\'' {
            if let Some(end) = char_literal_end(bytes, i) {
                line += bytes[i..end].iter().filter(|byte| **byte == b'\n').count();
                i = end;
                continue;
            }
        }
        if is_ident_start(bytes[i]) {
            let start = i;
            i += 1;
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            out.push(Token {
                kind: TokenKind::Ident(source[start..i].to_string()),
                line,
            });
            continue;
        }
        out.push(Token {
            kind: TokenKind::Punct(bytes[i] as char),
            line,
        });
        i += 1;
    }
    out
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    if bytes.get(i) == Some(&b'b') || bytes.get(i) == Some(&b'c') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let hashes_start = i;
    while bytes.get(i) == Some(&b'#') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let hashes = i - hashes_start;
    i += 1;
    let mut newlines = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            newlines += 1;
        }
        if bytes[i] == b'"'
            && bytes.get(i + 1..i + 1 + hashes) == Some(&vec![b'#'; hashes][..])
        {
            return Some((i + 1 + hashes, newlines));
        }
        i += 1;
    }
    Some((bytes.len(), newlines))
}

fn char_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < bytes.len() && bytes[i] != b'\n' {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == b'\'' {
            return Some(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic() || byte >= 0x80
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

fn punct(token: &Token, expected: char) -> bool {
    token.kind == TokenKind::Punct(expected)
}

fn ident(token: &Token, expected: &str) -> bool {
    matches!(&token.kind, TokenKind::Ident(actual) if actual == expected)
}

fn matching(tokens: &[Token], start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0;
    for (offset, token) in tokens[start..].iter().enumerate() {
        if punct(token, open) {
            depth += 1;
        } else if punct(token, close) {
            depth -= 1;
            if depth == 0 {
                return Some(start + offset);
            }
        }
    }
    None
}

fn cfg_truth(tokens: &[Token]) -> Truth {
    let mut cursor = 0;
    let truth = parse_cfg_expr(tokens, &mut cursor);
    if cursor == tokens.len() {
        truth
    } else {
        Truth::Maybe
    }
}

fn parse_cfg_expr(tokens: &[Token], cursor: &mut usize) -> Truth {
    let Some(Token {
        kind: TokenKind::Ident(name),
        ..
    }) = tokens.get(*cursor)
    else {
        return Truth::Maybe;
    };
    let name = name.as_str();
    *cursor += 1;
    if name == "test" {
        return if *cursor == tokens.len()
            || tokens
                .get(*cursor)
                .is_some_and(|token| punct(token, ',') || punct(token, ')'))
        {
            Truth::No
        } else {
            Truth::Maybe
        };
    }
    if !matches!(name, "all" | "any" | "not")
        || !tokens.get(*cursor).is_some_and(|t| punct(t, '('))
    {
        return Truth::Maybe;
    }
    *cursor += 1;
    let mut values = Vec::new();
    while *cursor < tokens.len() && !punct(&tokens[*cursor], ')') {
        values.push(parse_cfg_expr(tokens, cursor));
        while *cursor < tokens.len()
            && !punct(&tokens[*cursor], ',')
            && !punct(&tokens[*cursor], ')')
        {
            *cursor += 1;
        }
        if tokens.get(*cursor).is_some_and(|t| punct(t, ',')) {
            *cursor += 1;
        }
    }
    if tokens.get(*cursor).is_some_and(|t| punct(t, ')')) {
        *cursor += 1;
    }
    match name {
        "all" if values.iter().any(|v| *v == Truth::No) => Truth::No,
        "all" if values.iter().all(|v| *v == Truth::Yes) => Truth::Yes,
        "any" if values.iter().any(|v| *v == Truth::Yes) => Truth::Yes,
        "any" if values.iter().all(|v| *v == Truth::No) => Truth::No,
        "not" if values.as_slice() == [Truth::Yes] => Truth::No,
        "not" if values.as_slice() == [Truth::No] => Truth::Yes,
        _ => Truth::Maybe,
    }
}

fn attribute_is_test_only(tokens: &[Token]) -> bool {
    if tokens.first().is_some_and(|token| ident(token, "test")) {
        return true;
    }
    if !tokens.first().is_some_and(|token| ident(token, "cfg"))
        || !tokens.get(1).is_some_and(|token| punct(token, '('))
    {
        return false;
    }
    let Some(end) = matching(tokens, 1, '(', ')') else {
        return false;
    };
    cfg_truth(&tokens[2..end]) == Truth::No
}

fn scan_source(path: &str, source: &str) -> Vec<Finding> {
    let tokens = lex(source);
    let mut findings = Vec::new();
    scan_scope(&tokens, 0, tokens.len(), false, "<module>", path, &mut findings);
    findings
}

fn scan_scope(
    tokens: &[Token],
    start: usize,
    end: usize,
    test_only: bool,
    current_item: &str,
    path: &str,
    findings: &mut Vec<Finding>,
) {
    let mut i = start;
    let mut pending_test = false;
    let mut pending_item: Option<String> = None;
    while i < end {
        if punct(&tokens[i], '#') && tokens.get(i + 1).is_some_and(|token| punct(token, '[')) {
            if let Some(close) = matching(tokens, i + 1, '[', ']') {
                pending_test |= attribute_is_test_only(&tokens[i + 2..close]);
                i = close + 1;
                continue;
            }
        }
        if let TokenKind::Ident(keyword) = &tokens[i].kind {
            if matches!(keyword.as_str(), "fn" | "mod" | "const" | "static" | "struct" | "enum" | "trait") {
                if let Some(Token {
                    kind: TokenKind::Ident(name),
                    ..
                }) = tokens.get(i + 1)
                {
                    pending_item = Some(format!("{keyword} {name}"));
                }
            } else if keyword == "impl" {
                pending_item = Some("impl".to_string());
            }
        }
        if !test_only
            && !pending_test
            && i + 2 < end
            && punct(&tokens[i], '.')
            && matches!(&tokens[i + 1].kind, TokenKind::Ident(name) if name == "unwrap" || name == "expect")
            && punct(&tokens[i + 2], '(')
        {
            let TokenKind::Ident(method) = &tokens[i + 1].kind else {
                unreachable!()
            };
            findings.push(Finding {
                path: path.to_string(),
                line: tokens[i + 1].line,
                method: method.clone(),
                item: current_item.to_string(),
            });
        }
        if punct(&tokens[i], '{') {
            let Some(close) = matching(tokens, i, '{', '}') else {
                return;
            };
            let item = pending_item.as_deref().unwrap_or(current_item);
            scan_scope(
                tokens,
                i + 1,
                close,
                test_only || pending_test,
                item,
                path,
                findings,
            );
            pending_test = false;
            pending_item = None;
            i = close + 1;
            continue;
        }
        if punct(&tokens[i], ';') {
            pending_test = false;
            pending_item = None;
        }
        i += 1;
    }
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn production_findings() -> Vec<Finding> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates/jetpack/src"), &mut files);
    files.sort();
    let mut findings = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .expect("scanned file stays beneath repository root")
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&file).expect("Jetpack source is readable");
        findings.extend(scan_source(&relative, &source));
    }
    findings
}

fn enforce_zero(findings: &[Finding]) -> Result<(), String> {
    if findings.is_empty() {
        return Ok(());
    }
    Err(findings
        .iter()
        .map(|finding| {
            format!(
                "{}:{}: .{}( in {}",
                finding.path, finding.line, finding.method, finding.item
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

#[test]
fn jetpack_production_has_no_unwrap_or_expect() {
    let findings = production_findings();
    assert!(
        enforce_zero(&findings).is_ok(),
        "production Jetpack unwrap/expect ceiling is zero:\n{}",
        enforce_zero(&findings).unwrap_err()
    );
}

#[test]
fn scanner_detects_production_and_seeded_gate_failure() {
    let findings = scan_source("fixture.rs", "fn live() { value.unwrap(); other.expect(\"x\"); }");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].item, "fn live");
    assert!(enforce_zero(&findings).is_err());
}

#[test]
fn scanner_ignores_nonexecuting_text_and_other_methods() {
    let source = r####"
fn live() {
    // value.unwrap();
    /* outer other.expect("x"); /* nested more.unwrap(); */ */
    let a = "text.unwrap()";
    let b = r#"raw.expect("x")"#;
    let c = b"bytes.unwrap()";
    let d = br##"raw bytes.expect("x")"##;
    let e = 'x';
    value.unwrap_or(1);
    value.unwrap_err();
}
"####;
    assert!(scan_source("fixture.rs", source).is_empty());
}

#[test]
fn scanner_excludes_tests_but_retains_following_production() {
    let source = r#"
#[cfg(test)]
mod tests { fn helper() { value.unwrap(); } }

#[test]
fn direct_test() { value.expect("test"); }

#[cfg(test)]
const TEST_ONLY: usize = value.unwrap();

fn after() { value.unwrap(); }
"#;
    let findings = scan_source("fixture.rs", source);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].item, "fn after");
}

#[test]
fn scanner_treats_unknown_cfg_conservatively() {
    let source = r#"
#[cfg(any(test, unix))]
fn maybe_production() { value.unwrap(); }

#[cfg(all(test, unix))]
fn definitely_test() { value.expect("test"); }

#[cfg(test = "custom")]
fn key_value_is_not_builtin_test_cfg() { value.expect("production"); }

#[cfg(any(test = "custom", test))]
fn nested_key_value_is_not_builtin_test_cfg() { value.unwrap(); }
"#;
    let findings = scan_source("fixture.rs", source);
    assert_eq!(findings.len(), 3);
    assert_eq!(findings[0].item, "fn maybe_production");
    assert_eq!(
        findings[1].item,
        "fn key_value_is_not_builtin_test_cfg"
    );
    assert_eq!(
        findings[2].item,
        "fn nested_key_value_is_not_builtin_test_cfg"
    );
}

#[cfg(unix)]
#[test]
fn file_walk_does_not_follow_outside_or_cyclic_symlinks() {
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "jetpack-unwrap-ratchet-{}-{nonce}",
        std::process::id()
    ));
    let scan_root = base.join("scan");
    let outside = base.join("outside");
    fs::create_dir_all(&scan_root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(scan_root.join("safe.rs"), "fn safe() {}\n").unwrap();
    fs::write(outside.join("escape.rs"), "fn escaped() { value.unwrap(); }\n").unwrap();
    symlink(&outside, scan_root.join("outside-link")).unwrap();
    symlink(&scan_root, scan_root.join("cycle")).unwrap();

    let mut files = Vec::new();
    collect_rs_files(&scan_root, &mut files);
    files.sort();

    assert_eq!(files, vec![scan_root.join("safe.rs")]);
    assert!(outside.join("escape.rs").is_file());
    fs::remove_dir_all(&base).unwrap();
}
