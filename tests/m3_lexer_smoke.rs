#[test]
fn enum_is_keyword() {
    let (toks, diags) = jet::jeter::jet("enum x { A; }");
    assert!(diags.is_empty(), "{diags:?}");
    assert!(matches!(toks[0].kind, jet::jeter::TokKind::KwEnum), "{:?}", toks[0].kind);
}
