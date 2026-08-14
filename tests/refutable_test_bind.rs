//! D-CHOOSE-TEST1=A: subject-first refutable test-bind.

const VALID: &str = r#"
fn parse_age() => Int ? String {
    return .Ok(42)
}

fn run() {
    parse_age() == .Ok(age) ?? return
    print(age)
}
"#;

#[test]
fn parser_accepts_subject_first_test_bind_and_rejects_pattern_left_route() {
    let (tokens, lex_diags) = jet::Lexer::lex(VALID);
    assert!(lex_diags.is_empty(), "valid test-bind lexed with diagnostics: {lex_diags:#?}");
    jet::Parser::parse(&tokens).expect("subject-first test-bind should parse");

    let (tokens, lex_diags) = jet::Lexer::lex(
        "fn run() { .Ok(age) :: parse_age() ?? return }\n",
    );
    assert!(lex_diags.is_empty(), "retired spelling lexed with diagnostics: {lex_diags:#?}");
    assert!(
        jet::Parser::parse(&tokens).is_err(),
        "retired pattern-left refutable bind must not gain a parser path"
    );
}

#[test]
fn sema_accepts_diverging_test_bind_and_keeps_the_name_afterward() {
    let output = jet::compile(VALID).expect("diverging test-bind should compile");
    assert!(output.rust.contains("age"), "generated program lost the bound name");
}

#[test]
fn sema_rejects_a_non_diverging_test_bind_route() {
    let source = r#"
fn parse_age() => Int ? String { return .Ok(42) }
fn run() {
    parse_age() == .Ok(age) ?? 0
}
"#;
    let diagnostics = jet::check_for_eval(source, "refutable_test_bind.jet");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0405"),
        "non-diverging test-bind route must use registered E0405: {diagnostics:#?}"
    );
}

#[test]
fn optional_shape_test_does_not_make_ambient_err_available() {
    let source = r#"
fn maybe() => Int? { return null }
fn run() {
    maybe() == .Val(value) ?? panic(err)
}
"#;
    let diagnostics = jet::check_for_eval(source, "refutable_test_bind_optional.jet");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0408"),
        "optional shape miss must reject ambient err with E0408: {diagnostics:#?}"
    );
}
