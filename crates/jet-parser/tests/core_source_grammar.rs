use jet_parser::{Lexer, Parser};

#[test]
fn civil_core_internals_use_the_trusted_loader_lexing_contract() {
    let source = include_str!("../../../Core/time/time.jet");
    // Embedded Core uses generated lexing; checking this file as a user entry
    // must not grant access to the reserved namespace.
    let (tokens, diagnostics) = Lexer::lex_generated(source);
    assert!(diagnostics.is_empty(), "trusted Core lexing: {diagnostics:#?}");
    Parser::parse(&tokens).expect("civil Core source must parse");
    let (_, diagnostics) = Lexer::lex("fn __wall_sample() -> Int { 0 }");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0067"));
}

#[test]
fn system_process_and_files_internals_parse_as_trusted_core() {
    for (name, source) in [
        ("sys", include_str!("../../../Core/sys/sys.jet")),
        ("process", include_str!("../../../Core/process/process.jet")),
        ("files", include_str!("../../../Core/files/files.jet")),
    ] {
        let (tokens, diagnostics) = Lexer::lex_generated(source);
        assert!(diagnostics.is_empty(), "{name} trusted lexing: {diagnostics:#?}");
        Parser::parse(&tokens).unwrap_or_else(|diagnostics| panic!("{name} trusted parsing: {diagnostics:#?}"));
    }
}
