#[test]
fn enum_is_keyword() {
    let (toks, diags) = jet::lexer::lex("enum x { A; }");
    assert!(diags.is_empty(), "{diags:?}");
    assert!(
        matches!(toks[0].kind, jet::lexer::TokKind::KwEnum),
        "{:?}",
        toks[0].kind
    );
}

#[test]
fn dot_zero_in_statement_lexes_as_dot_then_int() {
    let (toks, diags) = jet::lexer::lex("fn main() { val x = p.0; }");
    assert!(diags.is_empty(), "{diags:?}");
    let dot = toks
        .iter()
        .position(|t| matches!(t.kind, jet::lexer::TokKind::Dot))
        .expect("dot");
    assert!(
        matches!(toks[dot + 1].kind, jet::lexer::TokKind::Int(0)),
        "{:?}",
        toks[dot + 1].kind
    );
}
