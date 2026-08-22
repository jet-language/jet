//! D-CHOOSE-TEST1=A: subject-first refutable test-bind.

#[path = "tir_support/mod.rs"]
mod tir_support;
mod common;

const EXAMPLE_OUTPUT: &str = "7\n7\ninvalid\ninvalid\n";

const VALID: &str = r#"
fn parse_age() Int String! {
    return .Ok(42)
}

fn run() {
    parse_age() == .Ok(age) ?? panic("invalid age")
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
fn parse_age() Int String! { return .Ok(42) }
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
fn a_test_without_a_route_does_not_bind_pattern_names() {
    let source = r#"
fn parse_age() Int String! { return .Ok(42) }
fn run() {
    matched :: parse_age() == .Ok(age)
    print(age)
}
"#;
    let diagnostics = jet::check_for_eval(source, "refutable_test_bind_no_route.jet");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0107"),
        "a pattern test without `??` must not bind `age`: {diagnostics:#?}"
    );
}

#[test]
fn optional_shape_test_does_not_make_ambient_err_available() {
    let source = r#"
fn maybe() Int? { return null }
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

#[test]
fn user_enum_capture_and_miss_route_match_all_tiers() {
    let source = r#"
enum Choice {
    First(Int)
    Second
}

fn select(choice: Choice) Int {
    choice == .First(value) ?? return 0
    return value
}

fn run() {
    print(select(Choice.First(7)))
    print(select(Choice.Second))
}
"#;
    tir_support::assert_tiers_agree("refutable-user-enum", source, "7\n0\n");
}

#[test]
fn example_matches_aot_default_jit_and_interpreter() {
    tir_support::assert_example_cli_tiers_agree(
        "patterns/refutable_test_bind",
        EXAMPLE_OUTPUT,
    );
}
