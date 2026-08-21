use jet::{AST, Formatter, Lexer, Parser, Syntax};

#[test]
fn user_policy_declaration_and_custom_chain_parse_together() {
    let source = r#"
pub policy audit(topic: String) {
    wrap(call) { return call() }
}

#Policy(audit("users.load"))
fn load_user() Int :> 7
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
