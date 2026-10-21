use jet_parser::{Lexer, Parser};

fn parse_trusted_core(name: &str, source: &str) {
    // Use the module loader's offset-preserving package-header mask, not an
    // ad hoc time-specific prefix strip. The body still uses trusted lexing.
    let source = jet_pkg_model::Package::mask_inline_package_source(source)
        .unwrap_or_else(|error| panic!("{name} package header: {error:?}"))
        .0;
    let (tokens, diagnostics) = Lexer::lex_generated(&source);
    assert!(diagnostics.is_empty(), "{name} trusted lexing: {diagnostics:#?}");
    Parser::parse_for_check_with_source(&tokens, &source)
        .unwrap_or_else(|diagnostics| panic!("{name} trusted parsing: {diagnostics:#?}"));
}

#[test]
fn civil_core_internals_use_the_trusted_loader_lexing_contract() {
    let source = include_str!("../../../Core/Time/Time.jet");
    // Embedded Core uses generated lexing; checking this file as a user entry
    // must not grant access to the reserved namespace.
    parse_trusted_core("time", source);
    let (_, diagnostics) = Lexer::lex("fn __wall_sample() -> Int { 0 }");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0067"));
}

#[test]
fn system_process_and_files_internals_parse_as_trusted_core() {
    for (name, source) in [
        ("sys", include_str!("../../../Core/Sys/Sys.jet")),
        ("process", include_str!("../../../Core/Process/Process.jet")),
        ("files", include_str!("../../../Core/Files/Files.jet")),
    ] {
        parse_trusted_core(name, source);
    }
}

#[test]
fn terminal_policy_internals_parse_as_trusted_core() {
    let source = include_str!("../../../Core/Term/Term.jet");
    let (tokens, diagnostics) = Lexer::lex_generated(source);
    assert!(diagnostics.is_empty(), "terminal trusted lexing: {diagnostics:#?}");
    Parser::parse(&tokens).expect("terminal policy must parse");
}

#[test]
fn raylib_and_font_host_policies_parse_as_trusted_core() {
    for (name, source) in [
        ("raylib", include_str!("../../../Core/Game/Raylib.jet")),
        ("font", include_str!("../../../Core/Font/Font.jet")),
    ] {
        let (tokens, diagnostics) = Lexer::lex_generated(source);
        assert!(diagnostics.is_empty(), "{name} trusted lexing: {diagnostics:#?}");
        Parser::parse(&tokens).unwrap_or_else(|diagnostics| panic!("{name} trusted parsing: {diagnostics:#?}"));
    }
}

