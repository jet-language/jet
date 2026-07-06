use std::fs;
use std::path::PathBuf;

#[test]
fn lexer_api_returns_stable_value_tokens() {
    let src = "fn run() {\n    print(\"hi\")\n}\n";
    let lexed = jet::Compiler::lex_source(src);
    assert_eq!(lexed.api_version, jet::Compiler::API_VERSION);
    assert!(lexed.diagnostics.is_empty());
    assert!(lexed.tokens.iter().any(|t| {
        t.kind == "keyword.fn" && t.text == "fn" && t.start.line == 1 && t.start.column == 1
    }));
    assert!(lexed
        .tokens
        .iter()
        .any(|t| t.kind == "identifier" && t.text == "run"));
    assert_eq!(lexed.tokens.last().map(|t| t.kind), Some("eof"));
}

#[test]
fn parser_api_returns_read_only_syntax_summary() {
    let src = "struct User {\n    name: String\n}\n\nfn run() {\n    print(\"ok\")\n}\n";
    let tree = jet::Compiler::parse_source(src);
    assert!(tree.diagnostics.is_empty());
    assert!(tree.items.iter().any(|n| {
        n.kind == jet::Compiler::SyntaxNodeKind::Struct && n.name.as_deref() == Some("User")
    }));
    assert!(tree.items.iter().any(|n| {
        n.kind == jet::Compiler::SyntaxNodeKind::Function && n.name.as_deref() == Some("run")
    }));
}

#[test]
fn parser_api_reports_diagnostics_as_values() {
    let tree = jet::Compiler::parse_source("fn run( {\n");
    assert!(!tree.diagnostics.is_empty());
    assert!(tree.diagnostics.iter().any(|d| d.code == "E0003"));
}

#[test]
fn check_file_api_includes_semindex_for_clean_program() {
    let path = fixture_file(
        "compiler_api_clean.jet",
        "fn helper() -> Int {\n    return 41\n}\n\nfn run() {\n    print(helper() + 1)\n}\n",
    );
    let checked = jet::Compiler::check_file(&path);
    assert!(
        checked.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        checked.diagnostics
    );
    let sem = checked.semantic_index.expect("clean file has semindex");
    assert_eq!(sem.schema_version, jet_semindex::SCHEMA_VERSION);
    assert!(sem.definitions.iter().any(|d| d.name == "run"));
    assert!(sem.definitions.iter().any(|d| d.name == "helper"));
    assert!(sem.calls.iter().any(|c| c.callee == "helper"));
}

#[test]
fn check_file_api_keeps_semindex_absent_when_errors_exist() {
    let path = fixture_file(
        "compiler_api_bad.jet",
        "fn run() {\n    missing_name()\n}\n",
    );
    let checked = jet::Compiler::check_file(&path);
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.severity == jet::Compiler::DiagnosticSeverity::Error));
    assert!(checked.semantic_index.is_none());
}

#[test]
fn source_map_api_reads_generated_rust_markers() {
    let rust = "// jet:source-map source=input.jet\nfn main() {\n    // jet:line 7\n    let x = 1;\n    // jet:line 8\n}\n";
    let map = jet::Compiler::source_map_from_generated_rust(rust);
    assert_eq!(map.sources, vec!["input.jet".to_string()]);
    assert_eq!(map.generated_lines.len(), 2);
    assert_eq!(map.generated_lines[0].generated_line, 3);
    assert_eq!(map.generated_lines[0].source.as_deref(), Some("input.jet"));
    assert_eq!(map.generated_lines[0].source_line, 7);
}

fn fixture_file(name: &str, src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jet_compiler_api_{}_{}",
        std::process::id(),
        name.trim_end_matches(".jet")
    ));
    fs::create_dir_all(&dir).expect("create temp fixture dir");
    let path = dir.join(name);
    fs::write(&path, src).expect("write temp fixture");
    path
}
