mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
fn single_bar_is_alternatives_only_and_flow_pipe_has_no_foreign_guess() {
    let bit_or = jet::Compiler::parse_source(
        "fn run() {\n    left :: 1\n    right :: 2\n    value :: left | right\n}\n",
    );
    assert!(
        bit_or.diagnostics.iter().any(|diag| diag.code == "E0003"),
        "D-SHAPE-PIPE1=C rejects a general single-bar expression with E0003"
    );

    let pipe_closure = jet::Compiler::parse_source("fn run() {\n    f :: |x| x + 1\n}\n");
    assert!(
        pipe_closure.diagnostics.iter().any(|diag| diag.code == "E0003")
            && pipe_closure.diagnostics.iter().all(|diag| diag.code != "E0033"),
        "pipe-closure-shaped input must be ordinary E0003: {:?}",
        pipe_closure.diagnostics
    );

    let flow = jet::compile("fn run() {\n    value :: 1 |> print\n}\n")
        .expect_err("`|>` stays unassigned");
    assert!(flow.iter().any(|diag| diag.code == "E0003"), "{flow:?}");
    assert!(flow.iter().all(|diag| diag.code != "E0033"), "{flow:?}");
    assert!(
        flow.iter().all(|diag| {
            !diag.what.contains("pipeline")
                && !diag.why.contains("pipeline")
                && !diag.fix.contains("pipeline")
        }),
        "an unassigned token must not advertise a future flow alias: {flow:?}"
    );

    let alternatives = jet::Compiler::parse_source(
        "enum State { Ready Waiting Done }\nfn run() {\n    state :: State.Ready\n    if state == {\n        .Ready | .Waiting -> { print(\"open\") }\n        .Done -> { print(\"done\") }\n    }\n}\n",
    );
    assert!(
        alternatives.diagnostics.is_empty(),
        "pattern alternatives remain legal: {:?}",
        alternatives.diagnostics
    );

    let boolean_or = jet::Compiler::parse_source(
        "fn run() {\n    if true || false { print(\"ok\") }\n}\n",
    );
    assert!(
        boolean_or.diagnostics.is_empty(),
        "`||` keeps its boolean-or meaning: {:?}",
        boolean_or.diagnostics
    );
}

#[test]
fn check_file_api_includes_semindex_for_clean_program() {
    let path = fixture_file(
        "compiler_api_clean.jet",
        "fn helper() => Int {\n    return 41\n}\n\nfn run() {\n    print(helper() + 1)\n}\n",
    );
    let checked = jet::Compiler::check_file(&path);
    assert!(
        checked.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        checked.diagnostics
    );
    let sem = checked.semantic_index.expect("clean file has semindex");
    assert_eq!(sem.schema_version, jet_semindex::SCHEMA_VERSION);
    assert_eq!(
        sem.source_digest,
        jet::SHA256::sha256_hex(
            b"fn helper() => Int {\n    return 41\n}\n\nfn run() {\n    print(helper() + 1)\n}\n",
        )
    );
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

#[test]
fn compiler_api_json_mirrors_are_schema_versioned() {
    let source = "fn run() { print(\"ok\") }\n";
    let lex = jet::Compiler::lex_source_json(source);
    assert!(lex.starts_with("{\"schema_version\":1,\"api_version\":1,\"operation\":\"lex\""));
    assert!(lex.contains("\"tokens\":["));
    let parse = jet::Compiler::parse_source_json(source);
    assert!(parse.contains("\"operation\":\"parse\""));
    assert!(parse.contains("\"kind\":\"function\""));

    let path = fixture_file("compiler_api_json.jet", source);
    let check = jet::Compiler::check_file_json(&path);
    assert!(check.contains("\"operation\":\"check\""));
    assert!(check.contains("\"semantic_index\":{\"schema_version\":1"));
    assert!(!check.contains("semantic_index\\\""), "semantic facts must not be JSON strings");
    let map = jet::Compiler::source_map_json(
        "// jet:source-map source=input.jet\n// jet:line 3\n",
    );
    assert!(map.contains("\"operation\":\"source_map\""));
    assert!(map.contains("\"generated_line\":2"));
}

#[test]
fn compiler_check_json_is_the_typed_value_not_a_second_shape() {
    let source = "fn run() { print(\"same\") }\n";
    let path = fixture_file("compiler_api_check_shape.jet", source);
    let parsed = jet::Compiler::eval_core_call(
        "core.compiler",
        "parse",
        vec![jet::AST::CtValue::Str(source.to_string())],
        jet::Diagnostics::Span::new(0, 0),
    )
    .unwrap()
    .unwrap();
    let jet::AST::CtValue::Present(parsed) = parsed else {
        panic!("parse must return a typed success value")
    };
    let checked = jet::Compiler::eval_core_call(
        "core.compiler",
        "check",
        vec![*parsed],
        jet::Diagnostics::Span::new(0, 0),
    )
    .unwrap()
    .unwrap();
    let jet::AST::CtValue::Present(checked) = checked else {
        panic!("check must return a typed success value")
    };
    let typed_json = checked.to_json();
    let cli_json = jet::Compiler::check_file_json(&path);
    assert!(
        cli_json.contains(&format!("\"value\":{typed_json}")),
        "CLI check must serialize the exact typed CompilerChecked value: {cli_json}"
    );
    assert!(typed_json.contains("\"functions\":"));
    assert!(typed_json.contains("\"effects\":"));
    assert!(typed_json.contains("\"semantic_index\":{"));
}

#[test]
fn compiler_check_failure_keeps_semantic_index_absent() {
    let source = "fn run() { missing_name() }\n";
    let checked = jet::Compiler::eval_core_call(
        "core.compiler",
        "check",
        vec![jet::AST::CtValue::Struct {
            type_name: "CompilerSyntaxTree".to_string(),
            fields: vec![
                ("schema_version".to_string(), jet::AST::CtValue::Int(1)),
                ("source".to_string(), jet::AST::CtValue::Str(source.to_string())),
            ],
        }],
        jet::Diagnostics::Span::new(0, 0),
    )
    .unwrap()
    .unwrap();
    let jet::AST::CtValue::Present(checked) = checked else {
        panic!("check must return a typed success value")
    };
    assert!(checked.to_json().contains("\"semantic_index\":null"));
}

#[test]
fn compiler_api_cli_returns_the_same_json_envelope() {
    let path = fixture_file("compiler_api_cli.jet", "fn run() { print(\"cli\") }\n");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "compiler", "parse", path.to_str().unwrap()])
        .output()
        .expect("run compiler inspection command");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"schema_version\":1"));
    assert!(stdout.contains("\"operation\":\"parse\""));
    assert!(stdout.contains("\"name\":\"run\""));
    assert_eq!(
        stdout.trim(),
        jet::Compiler::parse_source_json("fn run() { print(\"cli\") }\n")
    );
}

#[test]
fn compiler_cli_mirrors_each_read_only_operation_exactly() {
    let source = "fn run() { print(\"same\") }\n";
    let path = fixture_file("compiler_api_differential.jet", source);
    for operation in ["lex", "parse", "check", "source-map"] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["inspect", "compiler", operation, path.to_str().unwrap()])
            .output()
            .expect("run compiler inspection command");
        assert!(
            output.status.success(),
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout).expect("compiler JSON is UTF-8");
        let expected = match operation {
            "lex" => jet::Compiler::lex_source_json(source),
            "parse" => jet::Compiler::parse_source_json(source),
            "check" => jet::Compiler::check_file_json(&path),
            "source-map" => jet::Compiler::source_map_json(source),
            _ => unreachable!(),
        };
        assert_eq!(actual.trim(), expected, "{operation}");
    }
}

#[test]
fn compiler_api_is_compile_time_only() {
    let diagnostics = jet::compile(
        "use core.compiler as compiler\nfn run() { compiler.lex(\"fn run() {{}}\") }\n",
    )
    .expect_err("the compiler API must not become a runtime capability");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0956"),
        "expected compile-time-only diagnostic, got {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.what.contains("compile-time only")),
        "diagnostic must teach the phase boundary: {diagnostics:?}"
    );
}

#[test]
fn compiler_api_failures_are_typed_and_schema_checked() {
    let bad_shape = jet::Compiler::eval_core_call(
        "core.compiler",
        "check",
        vec![jet::AST::CtValue::Str("not-a-syntax-tree".to_string())],
        jet::Diagnostics::Span::new(0, 1),
    )
    .expect("compiler callback handles its module")
    .expect("failure is a typed Result value, not a host diagnostic");
    let jet::AST::CtValue::Failed(jet::AST::CtReport::Told(error)) = bad_shape else {
        panic!("expected CompilerError result, got {bad_shape:?}");
    };
    assert!(matches!(
        error.as_ref(),
        jet::AST::CtValue::Struct { type_name, fields }
            if type_name == "CompilerError"
                && fields.iter().any(|(name, value)| name == "code" && value == &jet::AST::CtValue::Str("E0956".into()))
    ));

    let stale_tree = jet::AST::CtValue::Struct {
        type_name: "CompilerSyntaxTree".to_string(),
        fields: vec![
            ("schema_version".to_string(), jet::AST::CtValue::Int(999)),
            ("source".to_string(), jet::AST::CtValue::Str("fn run() {}".to_string())),
        ],
    };
    let stale = jet::Compiler::eval_core_call(
        "core.compiler",
        "check",
        vec![stale_tree],
        jet::Diagnostics::Span::new(0, 1),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(stale, jet::AST::CtValue::Failed(jet::AST::CtReport::Told(_))));
}

#[test]
fn compiler_cli_unknown_operation_uses_structured_error_object() {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "compiler", "unknown", "missing.jet"])
        .output()
        .expect("run compiler operation error");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"error\":{\"code\":\"E0956\""));
    assert!(!stdout.contains("\"error\":\""), "error must be an object");
}

#[test]
fn an_unselected_runtime_named_build_cannot_use_the_compiler_api() {
    let diagnostics = jet::compile(
        "use core.compiler as compiler\nfn build() { print(compiler.lex(\"fn run() {{}}\")) }\nfn run() {}\n",
    )
    .expect_err("an ordinary runtime function named build must not gain build authority");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0956"),
        "expected compile-time-only diagnostic, got {diagnostics:?}"
    );
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
