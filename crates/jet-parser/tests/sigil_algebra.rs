use jet_parser::{Lexer, Parser};

#[test]
fn retired_arithmetic_repairs_cover_complete_operands_and_precedence() {
    for (source, old, replacement) in [
        ("fn run() { print(state.first ~| rotate(value, 3)) }", "state.first ~| rotate(value, 3)", "(state.first).bit_xor(rotate(value, 3))"),
        ("fn run() { print(a + b & c * d) }", "a + b & c * d", "(a + b).bit_and(c * d)"),
        ("fn run() { print(a ^ b ^ c) }", "b ^ c", "(b).pow(c)"),
        ("fn run() { print(\"{call(x) & mask}\") }", "call(x) & mask", "(call(x)).bit_and(mask)"),
        ("fn run() { n := 1\n n ~|= next()\n }", "n ~|= next()", "n = (n).bit_xor(next())"),
    ] {
        let (tokens, _) = Lexer::lex(source);
        let diagnostics = match Parser::parse_for_check_with_source(&tokens, source) {
            Ok((_, diagnostics)) => diagnostics,
            Err(diagnostics) => diagnostics,
        };
        let edits: Vec<_> = diagnostics.iter().filter(|d| d.code == "E0398").filter_map(|d| d.edit.as_ref()).collect();
        assert!(edits.iter().any(|edit| &source[edit.span.start..edit.span.end] == old && edit.new_text == replacement), "{diagnostics:?}");
    }
}

#[test]
fn ownership_marks_and_alternatives_do_not_retire() {
    let source = "enum State { On Off }\nfn edit(n: &Int) {}\nfn run() { n := 1\n edit(&n)\n value :: State.On\n if value == .On | .Off -> print(!false)\n }";
    let (tokens, _) = Lexer::lex(source);
    let diagnostics = match Parser::parse_for_check_with_source(&tokens, source) {
        Ok((_, diagnostics)) => diagnostics,
        Err(diagnostics) => diagnostics,
    };
    assert!(!diagnostics.iter().any(|d| d.code == "E0398"), "{diagnostics:?}");
}

#[test]
fn distinct_markers_select_the_type_site_before_module_bindings() {
    for source in [
        "#Numeric Meters :: distinct Float",
        "#Numeric\nMeters :: distinct Float",
        include_str!("../../../tests/run/tir_language_features/distinct_types.jet"),
    ] {
        let (tokens, lexer_diagnostics) = Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:#?}");
        let (program, diagnostics) = Parser::parse_for_check_with_source(&tokens, source)
            .unwrap_or_else(|diagnostics| panic!("{diagnostics:#?}"));
        assert!(
            !diagnostics.iter().any(|diagnostic| diagnostic.code == "E0355"),
            "{diagnostics:#?}"
        );
        let meters = program.items.iter().find_map(|item| match item {
            jet_parser::AST::Item::Distinct(declaration) if declaration.name == "Meters" => Some(declaration),
            _ => None,
        }).expect("a marked distinct declaration is a type, not a constant");
        assert!(meters.type_markers.iter().any(|marker| marker.name == "Numeric"));
    }
}

#[test]
fn distinct_markers_do_not_widen_constant_marker_sites() {
    let source = "#Numeric ANSWER :: 42";
    let (tokens, _) = Lexer::lex(source);
    let diagnostics = match Parser::parse_for_check_with_source(&tokens, source) {
        Ok((_, diagnostics)) => diagnostics,
        Err(diagnostics) => diagnostics,
    };
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0355"), "{diagnostics:#?}");
}
