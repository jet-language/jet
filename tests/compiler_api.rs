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
fn postfix_in_lexes_as_a_member_identifier() {
    let lexed = jet::Compiler::lex_source(
        "fn run() {\n    value :: duration.in(.Seconds)\n    loop item in [1] -> print(item)\n}\n",
    );
    assert!(lexed.diagnostics.is_empty(), "unexpected lexer diagnostics: {:?}", lexed.diagnostics);
    assert!(lexed
        .tokens
        .iter()
        .any(|token| token.kind == "identifier" && token.text == "in"));
    assert!(lexed
        .tokens
        .iter()
        .any(|token| token.kind == "keyword.in" && token.text == "in"));
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
fn reserved_in_diagnostic_teaches_postfix_member_carve_out() {
    let tree = jet::Compiler::parse_source("fn run() {\n    in :: 1\n}\n");
    let diagnostic = tree
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0003")
        .expect("bare `in` should remain reserved");
    assert!(
        diagnostic.fix.contains("after `.` it is allowed as a member name"),
        "diagnostic must teach D-TIME-IN1=C: {}",
        diagnostic.fix
    );
}

#[test]
fn single_bar_is_bitwise_or() {
    let parsed = jet::Compiler::parse_source(
        "fn run() {\n    left :: 1\n    right :: 2\n    value :: left | right\n}\n",
    );
    assert!(
        parsed.diagnostics.is_empty(),
        "D-BITOREXPR1=A admits value `|` as bitwise OR: {:?}",
        parsed.diagnostics
    );
}

#[test]
fn pipe_closure_shape_has_no_foreign_guess() {
    let parsed = jet::Compiler::parse_source("fn run() {\n    f :: |x| x + 1\n}\n");
    assert!(
        parsed.diagnostics.iter().any(|diag| diag.code == "E0003")
            && parsed.diagnostics.iter().all(|diag| diag.code != "E0033"),
        "pipe-closure-shaped input must be ordinary E0003: {:?}",
        parsed.diagnostics
    );
}

#[test]
fn flow_pipe_shape_has_no_foreign_guess() {
    let diagnostics = jet::compile("fn run() {\n    value :: 1 |> print\n}\n")
        .expect_err("`|>` stays unassigned");
    assert!(
        diagnostics.iter().any(|diag| diag.code == "E0003"),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diag| diag.code != "E0033"),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diag| {
            !diag.what.contains("pipeline")
                && !diag.why.contains("pipeline")
                && !diag.fix.contains("pipeline")
        }),
        "an unassigned token must not advertise a future flow alias: {diagnostics:?}"
    );
}

#[test]
fn pattern_alternatives_keep_single_bar() {
    let parsed = jet::Compiler::parse_source(
        "enum State { Ready Waiting Done }\nfn run() {\n    state :: State.Ready\n    if state == {\n        .Ready | .Waiting -> { print(\"open\") }\n        .Done -> { print(\"done\") }\n    }\n}\n",
    );
    assert!(
        parsed.diagnostics.is_empty(),
        "pattern alternatives remain legal: {:?}",
        parsed.diagnostics
    );
}

#[test]
fn double_bar_keeps_boolean_or() {
    let parsed =
        jet::Compiler::parse_source("fn run() {\n    if true || false { print(\"ok\") }\n}\n");
    assert!(
        parsed.diagnostics.is_empty(),
        "`||` keeps its boolean-or meaning: {:?}",
        parsed.diagnostics
    );
}

#[test]
fn check_file_api_includes_semindex_for_clean_program() {
    let path = fixture_file(
        "compiler_api_clean.jet",
        "fn helper() Int -[]> {\n    return 41\n}\n\nfn run() {\n    print(helper() + 1)\n}\n",
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
            b"fn helper() Int -[]> {\n    return 41\n}\n\nfn run() {\n    print(helper() + 1)\n}\n",
        )
    );
    assert!(sem.definitions.iter().any(|d| d.name == "run"));
    assert!(sem.definitions.iter().any(|d| d.name == "helper"));
    assert!(sem.calls.iter().any(|c| c.callee == "helper"));
}

#[test]
fn check_file_api_projects_arithmetic_policy_and_scope() {
    let source = "fn run() {\n    #Arithmetic(.Wrapping) {\n        value :: U8{250} + U8{10}\n        print(value)\n    }\n}\n";
    let path = fixture_file("compiler_api_arithmetic.jet", source);
    let checked = jet::Compiler::check_file(&path);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let sem = checked.semantic_index.expect("arithmetic semindex");
    let operation = sem.arithmetic.first().expect("arithmetic operation");
    assert_eq!(operation.operation, "add");
    assert_eq!(operation.policy, "Wrapping");
    assert!(operation.scope_span.start < operation.operation_span.start);
    assert!(operation.scope_span.end > operation.operation_span.end);
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
    assert!(lex.starts_with("{\"schema\":\"jet.report/v1\""));
    assert!(
        lex.contains("\"compiler\":{\"schema_version\":1,\"api_version\":1,\"operation\":\"lex\"")
    );
    assert!(lex.contains("\"tokens\":["));
    let parse = jet::Compiler::parse_source_json(source);
    assert!(parse.contains("\"operation\":\"parse\""));
    assert!(parse.contains("\"kind\":\"function\""));

    let path = fixture_file("compiler_api_json.jet", source);
    let check = jet::Compiler::check_file_json(&path);
    assert!(check.contains("\"operation\":\"check\""));
    assert!(check.contains("\"semantic_index\":{\"schema_version\":1"));
    assert!(
        !check.contains("semantic_index\\\""),
        "semantic facts must not be JSON strings"
    );
    let map = jet::Compiler::source_map_json("// jet:source-map source=input.jet\n// jet:line 3\n");
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
                (
                    "source".to_string(),
                    jet::AST::CtValue::Str(source.to_string()),
                ),
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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"schema\":\"jet.report/v1\""));
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
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0956"),
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
            (
                "source".to_string(),
                jet::AST::CtValue::Str("fn run() {}".to_string()),
            ),
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
    assert!(matches!(
        stale,
        jet::AST::CtValue::Failed(jet::AST::CtReport::Told(_))
    ));
}
#[test]
fn package_views_read_real_inputs_through_comptime_and_match_goldens() {
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_package_views_{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join(".jet")).expect("create package view fixture");
    fs::create_dir_all(root.join("config")).expect("create package config fixture");
    fs::write(
        root.join("package.jet"),
        r#"
name: "demo"
version: "1.2.3"
edition: "2028"
description: "typed package"
license: "MIT"
repository: "https://example.test/demo"
runtime: "hosted"
target: "native"
configs: ["config/build.jet"]
deps: {
    gitdep: {
        git: "https://build-user:build-secret@example.test/acme/tool?token=query-secret#private",
        tag: "v1",
    },
    local: ./deps/local,
}
"#,
    )
    .expect("write package manifest fixture");
    fs::write(
        root.join("config/build.jet"),
        r#"pub build :: Config{
    outputs: { app: Executable{ entry: run } }
}
"#,
    )
    .expect("write package config fixture");
    fs::write(
        root.join(".jet/lock"),
        r#"
version = 1
[root]
dependencies = ["gitdep"]
[[package]]
name = "gitdep"
version = "1.0.0"
source = { git = "https://lock-user:lock-secret@example.test/acme/tool?token=lock-secret", tag = "v1" }
locked = { rev = "deadbeef", tree-hash = "tree", last-modified = 42 }
fingerprint = "lock-fp"
content-hash = "lock-hash"
dependencies = []
layer = "hosted"
inferred-layer = "hosted"
"#,
    )
    .expect("write lock fixture");
    fs::write(
        root.join("env.jet"),
        r#"
module profile.base {
    packages: []
}
module profile.dev {
    extends: ["base"],
    packages: [],
    collisions: { "bin/editor": "editor@default" }
}
"#,
    )
    .expect("write profile fixture");

    let manifest = jet::Compiler::read_manifest(&root).expect("read manifest view");
    let package = jet::Compiler::read_package(&root).expect("read package view");
    let lock = jet::Compiler::read_lock(&root).expect("read lock view");
    let profiles = jet::Compiler::read_profiles(&root).expect("read profile view");
    assert_eq!(manifest.dependencies, package.dependencies);
    assert!(manifest.outputs.is_empty(), "manifest must stay uncomposed");
    assert_eq!(package.outputs.len(), 1, "package must include Config outputs");
    assert_eq!(package.outputs[0].name, "app");
    assert_eq!(package.outputs[0].kind, "executable");
    assert_eq!(package.outputs[0].entry.as_deref(), Some("run"));
    assert_eq!(
        manifest.dependencies[0].source,
        r#"{ git: "https://example.test/acme/tool", tag: "v1" }"#
    );
    assert_eq!(lock.packages[0].source_kind, "git");
    assert_eq!(profiles.profiles.len(), 2);

    let read = |operation| {
        jet::Compiler::eval_core_call(
            "core.compiler",
            operation,
            Vec::new(),
            jet::Diagnostics::Span::new(0, 0),
        )
        .expect("compiler callback handles package view")
        .expect("package view is present")
    };
    let (values, inputs) = jet::Comptime::with_package_read_context(&root, || {
        (
            read("manifest"),
            read("package"),
            read("lock"),
            read("profiles"),
        )
    });
    let json = |value: jet::AST::CtValue| match value {
        jet::AST::CtValue::Present(value) => value.to_json(),
        other => panic!("expected a present package view, got {other:?}"),
    };
    let (manifest_value, package_value, lock_value, profiles_value) = values;
    let manifest_json = json(manifest_value);
    let package_json = json(package_value);
    let lock_json = json(lock_value);
    let profiles_json = json(profiles_value);
    assert_eq!(
        manifest_json,
        r#"{"schema_version":1,"file":"package.jet","jet":null,"edition":"2028","description":"typed package","license":"MIT","repository":"https://example.test/demo","layer":"hosted","target":"native","dependencies":[{"name":"gitdep","source":"{ git: \"https://example.test/acme/tool\", tag: \"v1\" }"},{"name":"local","source":"./deps/local"}],"packages":[],"outputs":[],"build_profiles":[]}"#
    );
    assert_eq!(
        package_json,
        r#"{"schema_version":1,"file":"package.jet","jet":null,"edition":"2028","description":"typed package","license":"MIT","repository":"https://example.test/demo","layer":"hosted","target":"native","dependencies":[{"name":"gitdep","source":"{ git: \"https://example.test/acme/tool\", tag: \"v1\" }"},{"name":"local","source":"./deps/local"}],"packages":[],"outputs":[{"name":"app","kind":"executable","entry":"run"}],"build_profiles":[]}"#
    );
    assert_eq!(
        lock_json,
        r#"{"schema_version":1,"file":".jet/lock","version":1,"root_dependencies":["gitdep"],"packages":[{"name":"gitdep","version":"1.0.0","source_kind":"git","source":"tag = \"v1\"","revision":"deadbeef","fingerprint":"lock-fp","content_hash":"lock-hash","dependencies":[],"layer":"hosted","inferred_layer":"hosted"}]}"#
    );
    assert_eq!(
        profiles_json,
        r#"{"schema_version":1,"file":"env.jet","profiles":[{"name":"base","extends":[],"packages":[],"collisions":[],"sources":["profile.base"]},{"name":"dev","extends":["base"],"packages":[],"collisions":[{"key":"bin/editor","value":"editor@default"}],"sources":["profile.dev"]}]}"#
    );
    for projection in [&manifest_json, &package_json, &lock_json, &profiles_json] {
        for secret in [
            "build-user",
            "build-secret",
            "query-secret",
            "lock-user",
            "lock-secret",
        ] {
            assert!(!projection.contains(secret), "projection leaked {secret}: {projection}");
        }
        assert!(
            !projection.contains(root.to_str().unwrap()),
            "projection leaked authority path: {projection}"
        );
    }
    let expected_inputs = [
        ("package.jet", root.join("package.jet")),
        ("config/build.jet", root.join("config/build.jet")),
        (".jet/lock", root.join(".jet/lock")),
        ("env.jet", root.join("env.jet")),
    ];
    assert_eq!(inputs.len(), expected_inputs.len());
    for (relative, absolute) in expected_inputs {
        let input = inputs
            .iter()
            .find(|input| input.path == relative)
            .unwrap_or_else(|| panic!("missing consumed package input {relative}"));
        assert_eq!(
            input.hash,
            jet::SHA256::sha256_hex(&fs::read(absolute).expect("read consumed input"))
        );
    }
}

#[test]
fn inline_and_extracted_package_views_are_equivalent() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "jet_compiler_inline_carrier_equivalence_{}_{}",
        std::process::id(),
        stamp
    ));
    let inline_root = base.join("inline");
    let extracted_root = base.join("extracted");
    let body = r#"
name: "carrier-equivalence"
version: "1.2.3"
jet: ">=0.1.0"
edition: "2028"
description: "same typed facts"
license: "MIT"
repository: "https://example.test/carrier"
runtime: "hosted"
target: "native"
configs: ["config/build.jet"]
"#;
    for root in [&inline_root, &extracted_root] {
        fs::create_dir_all(root.join("config")).expect("create carrier fixture");
        fs::write(
            root.join("config/build.jet"),
            "pub build :: Config{ outputs: { app: Executable{ entry: run } } }\n",
        )
        .expect("write carrier config");
    }
    fs::write(
        inline_root.join("run.jet"),
        format!("package {{{body}}}\nfn run() {{}}\n"),
    )
    .expect("write inline carrier");
    fs::write(extracted_root.join("package.jet"), body).expect("write extracted manifest");

    let inline_manifest = jet::Compiler::read_manifest(&inline_root).expect("read inline manifest");
    let extracted_manifest =
        jet::Compiler::read_manifest(&extracted_root).expect("read extracted manifest");
    assert_eq!(inline_manifest, extracted_manifest);
    assert!(inline_manifest.outputs.is_empty());

    let inline_package = jet::Compiler::read_package(&inline_root).expect("read inline package");
    let extracted_package =
        jet::Compiler::read_package(&extracted_root).expect("read extracted package");
    assert_eq!(inline_package, extracted_package);
    assert_eq!(inline_package.outputs.len(), 1);
    assert_eq!(inline_package.outputs[0].name, "app");
}

#[test]
fn inline_package_config_inputs_invalidate_package_views() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_inline_config_invalidation_{}_{}",
        std::process::id(),
        stamp
    ));
    fs::create_dir_all(root.join("config")).expect("create inline invalidation fixture");
    fs::write(
        root.join("run.jet"),
        r#"package {
name: "inline-invalidation"
version: "1.0.0"
configs: ["config/build.jet"]
}
fn run() {}
"#,
    )
    .expect("write inline package");
    let config = root.join("config/build.jet");
    fs::write(
        &config,
        "pub build :: Config{ outputs: { app: Executable{ entry: run } } }\n",
    )
    .expect("write first package config");
    let (first_result, first_inputs) =
        jet::Comptime::with_package_read_context(&root, || jet::Compiler::read_package(&root));
    let first = first_result.expect("read first inline package");

    fs::write(
        &config,
        "pub build :: Config{ outputs: { app: Executable{ entry: check } } }\n",
    )
    .expect("write changed package config");
    let (second_result, second_inputs) =
        jet::Comptime::with_package_read_context(&root, || jet::Compiler::read_package(&root));
    let second = second_result.expect("read changed inline package");

    assert_ne!(first, second, "a Config edit must change completed package facts");
    let input_hash = |inputs: &[jet::AST::ComptimeInput], path: &str| {
        inputs
            .iter()
            .find(|input| input.path == path)
            .map(|input| input.hash.clone())
            .unwrap_or_else(|| panic!("missing package input {path}: {inputs:?}"))
    };
    assert_ne!(
        input_hash(&first_inputs, "config/build.jet"),
        input_hash(&second_inputs, "config/build.jet")
    );
    for inputs in [&first_inputs, &second_inputs] {
        assert!(inputs.iter().any(|input| input.path == "run.jet"));
        assert!(inputs.iter().any(|input| input.path == "config/build.jet"));
    }
}

#[test]
fn inline_package_authority_errors_redact_absolute_paths() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_inline_authority_redaction_{}_{}",
        std::process::id(),
        stamp
    ));
    fs::create_dir_all(&root).expect("create inline authority fixture");
    fs::write(
        root.join("run.jet"),
        r#"package {
name: "inline-authority-error"
version: "1.0.0"
configs: ["missing-config"]
}
fn run() {}
"#,
    )
    .expect("write invalid inline package");

    let error =
        jet::Compiler::read_package(&root).expect_err("missing inline Config must fail closed");
    assert_eq!(error.code, "E0956");
    assert_eq!(error.file, "package.jet");
    assert!(
        error.cause.contains("E1334") && error.cause.contains("invalid"),
        "inline authority failure must use the typed redacted cause: {error:?}"
    );
    assert!(
        !error.cause.contains(root.to_str().unwrap()),
        "inline authority failure leaked the root path: {error:?}"
    );
}

#[test]
fn inline_package_parse_errors_preserve_registered_diagnostic() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_inline_parse_error_{}_{}",
        std::process::id(),
        stamp
    ));
    fs::create_dir_all(&root).expect("create inline parse fixture");
    fs::write(
        root.join("run.jet"),
        r#"package {
version: "1.0.0"
unknown: "value"
}
fn run() {}
"#,
    )
    .expect("write malformed inline package");

    let error =
        jet::Compiler::read_package(&root).expect_err("malformed inline package must fail closed");
    assert_eq!(error.code, "E0956");
    assert_eq!(error.file, "package.jet");
    assert!(
        error.cause.contains("E1206") && error.cause.contains("unknown"),
        "inline parse failure must preserve the registered typed cause: {error:?}"
    );
    assert!(
        !error.cause.contains("E1362"),
        "inline parse failure must not replace the registered cause with a generic code: {error:?}"
    );
    assert!(
        !error.cause.contains(root.to_str().unwrap()),
        "inline parse failure leaked the authority root: {error:?}"
    );
}

#[test]
fn package_views_redact_repository_credentials_and_queries() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_repository_redaction_{}_{}",
        std::process::id(),
        stamp
    ));
    fs::create_dir_all(&root).expect("create repository redaction fixture");
    fs::write(
        root.join("package.jet"),
        r#"name: "repository-redaction"
version: "1.0.0"
repository: "https://repo-user:repo-secret@example.test/acme/repo?token=repo-query#private"
"#,
    )
    .expect("write hostile repository");

    let manifest = jet::Compiler::read_manifest(&root).expect("read redacted manifest");
    let package = jet::Compiler::read_package(&root).expect("read redacted package");
    assert_eq!(
        manifest.repository.as_deref(),
        Some("https://example.test/acme/repo")
    );
    assert_eq!(
        package.repository.as_deref(),
        Some("https://example.test/acme/repo")
    );
    for value in [manifest.repository.as_deref(), package.repository.as_deref()] {
        let value = value.expect("repository remains present");
        assert!(!value.contains("repo-user"));
        assert!(!value.contains("repo-secret"));
        assert!(!value.contains("repo-query"));
    }

    fs::write(
        root.join("package.jet"),
        "name: \"repository-safe\"\nversion: \"1.0.0\"\nrepository: \"https://example.test/acme/repo\"\n",
    )
    .expect("write safe repository");
    assert_eq!(
        jet::Compiler::read_manifest(&root)
            .expect("read safe repository")
            .repository
            .as_deref(),
        Some("https://example.test/acme/repo")
    );
}

#[test]
fn package_api_example_checks_and_runs_through_the_shared_evaluator() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/tooling/compiler_api_package");
    let expected = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/features/expected/tooling/compiler_api_package.out"),
    )
    .expect("read compiler API package expected output");
    let entry = root.join("run.jet");
    let entry_string = entry.to_string_lossy().into_owned();
    let project = jet::check_project_build_for_tier(
        &entry_string,
        jet::Policy::GateSet::default(),
        "dev",
        &std::collections::BTreeMap::new(),
        None,
        None,
    )
    .expect("package project build check should start")
    .expect("package Config should select the project build");
    assert!(project.build.is_some(), "project build check lost its BuildPlan");
    assert!(
        project.runtime.is_some(),
        "project build check lost the resolved runtime graph"
    );
    assert!(
        project.runtime_effect_facts.is_some(),
        "project build check lost runtime effect facts"
    );
    // This witness exercises the source carrier in explicit-file scope. The
    // package `app` Output is a Config contribution, so project-output proof
    // belongs to the build path above rather than a raw run.jet check.
    for target in [entry] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["check", target.to_str().expect("UTF-8 package target")])
            .current_dir(&root)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
            .expect("compiler API package check should start");
        assert!(
            output.status.success(),
            "compiler API package check failed for {}:\n{}",
            target.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("E0956"), "{target:?} emitted E0956: {stderr}");
        assert!(
            !stderr.contains("unbound"),
            "{target:?} emitted an unbound follow-on: {stderr}"
        );
        assert!(
            !stderr.contains("L0103"),
            "{target:?} reported a qualified compiler alias as unused: {stderr}"
        );
    }

    for args in [vec!["run"], vec!["run", "--interpret"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(&args)
            .current_dir(&root)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
            .expect("compiler API package run should start");
        assert!(
            output.status.success(),
            "compiler API package run {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).as_ref(),
            expected.as_str(),
            "compiler API package output diverged for {:?}",
            args
        );
    }
    if common::have_rustc() {
        let build_dir = root.join("build");
        let _ = fs::remove_dir_all(&build_dir);
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .arg("build")
            .current_dir(&root)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
            .expect("compiler API package AOT build should start");
        assert!(
            output.status.success(),
            "compiler API package AOT build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("L0103"),
            "compiler API package AOT build reported its qualified alias as unused:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let binary = build_dir.join("compiler_api_package");
        let run = Command::new(&binary)
            .output()
            .expect("compiler API package AOT binary should start");
        assert!(
            run.status.success(),
            "compiler API package AOT binary failed:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).as_ref(),
            expected.as_str(),
            "compiler API package AOT output diverged"
        );
        let _ = fs::remove_dir_all(build_dir);
    }
}


#[test]
fn package_views_remain_compile_time_only() {
    for operation in ["manifest", "package", "lock", "profiles"] {
        let source = format!(
            "use core.compiler as compiler\nfn run() {{ compiler.{operation}() }}\n"
        );
        let diagnostics =
            jet::compile(&source).expect_err("package views must not become runtime capabilities");
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "E0956"),
            "{operation} should report E0956: {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.what.contains("compile-time only")),
            "{operation} must teach the phase boundary: {diagnostics:?}"
        );
    }
}

#[test]
fn package_views_reject_config_paths_outside_the_pinned_root() {
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_package_view_escape_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create package escape fixture");
    fs::write(
        root.join("package.jet"),
        "name: \"escape\"\nversion: \"1.0.0\"\nconfigs: [\"../outside.jet\"]\n",
    )
    .expect("write escaping package fixture");

    let error =
        jet::Compiler::read_package(&root).expect_err("package view must reject escaping config");
    assert_eq!(error.code, "E0956");
    assert_eq!(error.file, "package.jet");
    assert!(
        error.cause.contains("E1334") && error.cause.contains("invalid"),
        "refusal must preserve a typed, actionable authority cause: {error:?}"
    );
    assert!(
        !error.cause.contains(root.to_str().unwrap()),
        "refusal cause leaked the authority root: {error:?}"
    );
}

#[test]
fn package_views_report_malformed_lock_as_typed_file_cause() {
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_package_view_malformed_lock_{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join(".jet")).expect("create malformed lock fixture");
    fs::write(
        root.join("package.jet"),
        "name: \"malformed-lock\"\nversion: \"1.0.0\"\n",
    )
    .expect("write malformed lock package");
    fs::write(
        root.join(".jet/lock"),
        "version = 1\n[[package]]\nversion = \"1.0.0\"\n",
    )
    .expect("write malformed lock");

    let error = jet::Compiler::read_lock(&root).expect_err("malformed lock must fail closed");
    assert_eq!(error.code, "E0956");
    assert_eq!(error.file, ".jet/lock");
    assert!(!error.message.is_empty());
    assert!(!error.cause.is_empty());

    let (value, inputs) = jet::Comptime::with_package_read_context(&root, || {
        jet::Compiler::eval_core_call(
            "core.compiler",
            "lock",
            Vec::new(),
            jet::Diagnostics::Span::new(0, 0),
        )
        .expect("compiler callback handles malformed package input")
        .expect("dispatcher returns a typed failure value")
    });
    let jet::AST::CtValue::Failed(jet::AST::CtReport::Told(payload)) = value else {
        panic!("malformed lock must be a told typed failure: {value:?}");
    };
    let payload = payload.to_json();
    assert!(payload.contains("\"code\":\"E0956\""), "{payload}");
    assert!(payload.contains("\"file\":\".jet/lock\""), "{payload}");
    assert!(payload.contains("\"cause\":\""), "{payload}");
    assert!(inputs.iter().any(|input| input.path == ".jet/lock"));
}

#[test]
fn package_views_reject_profile_imports_outside_the_pinned_root() {
    let root = std::env::temp_dir().join(format!(
        "jet_compiler_package_view_profile_escape_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create profile escape fixture");
    fs::write(
        root.join("env.jet"),
        r#"
module profile.base {
    imports: find("../outside")
    packages: []
}
"#,
    )
    .expect("write escaping profile fixture");

    let error =
        jet::Compiler::read_profiles(&root).expect_err("profile imports must stay below root");
    assert_eq!(error.code, "E0956");
    assert_eq!(error.file, "env.jet");
    assert!(
        error.cause.contains("E1331") || error.cause.contains("root"),
        "refusal must preserve the authority cause: {error:?}"
    );
    assert!(
        !error.cause.contains(root.to_str().unwrap()),
        "profile refusal cause leaked the authority root: {error:?}"
    );
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
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0956"),
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
