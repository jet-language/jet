use jet::{AST, Formatter, Lexer, Parser, Syntax};

mod common;

#[test]
fn user_policy_declaration_and_custom_chain_parse_together() {
    let source = r#"
pub policy audit(topic: String) {
    wrap(call) { return call() }
}

#Policy(audit("users.load"))
fn load_user() Int -> 7
"#;
    let (tokens, lexer_diagnostics) = Lexer::lex(source);
    assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
    let program = Parser::parse(&tokens).expect("user policy source should parse");

    let declaration = program
        .user_policy_declarations
        .first()
        .expect("the package policy declaration");
    assert!(declaration.is_pub);
    assert_eq!(declaration.name, "audit");
    assert_eq!(declaration.params.len(), 1);
    assert_eq!(declaration.params[0].name, "topic");

    let AST::Item::Func(function) = &program.items[0] else {
        panic!("expected the policy-decorated function");
    };
    let marker = function
        .markers
        .iter()
        .find(|marker| marker.name == Syntax::MARKER_POLICY)
        .expect("the callable policy marker");
    assert!(matches!(
        &marker.args[0],
        AST::Expr::Call(call) if call.name == "audit"
    ));

    let formatted = Formatter::format_source(source).expect("user policy source should format");
    assert!(formatted.contains("pub policy audit(topic: String)"));
    assert!(formatted.contains("wrap(call)"));
}

#[test]
fn user_policy_apply_keeps_a_callable_binding() {
    let dir = common::unique_tmp("jet_user_policy_apply");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        r#"pub policy audit(topic: String) {
    wrap(call) { return "{topic}:{call()}" }
}

fn load_user() String -> "ok"

fn run() {
    selected :: apply(audit("users.load"), load_user)
    print(selected())
}
"#,
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran {
            exit_code,
            stdout,
            stderr,
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout, "users.load:ok\n");
            assert_eq!(stderr, "");
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("interpreter rejected the user policy apply source: {diagnostics:#?}")
        }
    }
}
