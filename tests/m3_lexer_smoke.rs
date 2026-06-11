#[test]
fn enum_is_keyword() {
    let (toks, diags) = jet::lexer::lex("enum x { A; }");
    assert!(diags.is_empty(), "{diags:?}");
    assert!(matches!(toks[0].kind, jet::lexer::TokKind::KwEnum), "{:?}", toks[0].kind);
}
