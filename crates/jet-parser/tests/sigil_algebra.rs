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

#[test]
fn double_star_teaches_power_methods_and_repairs_whole_operands() {
    for (source, old, replacement) in [
        ("fn run() { print(2 ** 3) }", "2 ** 3", "(2).pow(3)"),
        ("fn run() { print((a + b) ** (c * d)) }", "(a + b) ** (c * d)", "((a + b)).pow((c * d))"),
        ("fn run() { print(base ** -1) }", "base ** -1", "(base).pow(-1)"),
        ("fn run() { print(a * b ** c) }", "b ** c", "(b).pow(c)"),
        ("fn run() { print(-2 ** 3) }", "2 ** 3", "(2).pow(3)"),
    ] {
        let (tokens, _) = Lexer::lex(source);
        let diagnostics = match Parser::parse_for_check_with_source(&tokens, source) {
            Ok((_, diagnostics)) => diagnostics,
            Err(diagnostics) => diagnostics,
        };
        let diagnostic = diagnostics.iter().find(|diagnostic| diagnostic.code == "E0396")
            .expect("double star retains its registered teaching diagnostic");
        assert!(diagnostic.what.contains("value.pow(exponent)"), "{diagnostic:#?}");
        assert!(diagnostic.why.contains("uses the `pow` method"), "{diagnostic:#?}");
        assert!(diagnostic.fix.contains("base.pow(exponent)"), "{diagnostic:#?}");
        assert!(!diagnostic.what.contains("`^`") && !diagnostic.fix.contains(" ^ "), "{diagnostic:#?}");
        let edit = diagnostic.edit.as_ref().expect("power repair covers both operands");
        assert_eq!(&source[edit.span.start..edit.span.end], old);
        assert_eq!(edit.new_text, replacement);
        let fixed = format!("{}{}{}", &source[..edit.span.start], edit.new_text, &source[edit.span.end..]);
        let (tokens, lexical) = Lexer::lex(&fixed);
        assert!(lexical.is_empty(), "{lexical:#?}");
        let (_, diagnostics) = Parser::parse_for_check_with_source(&tokens, &fixed)
            .unwrap_or_else(|diagnostics| panic!("{diagnostics:#?}"));
        assert!(!diagnostics.iter().any(|diagnostic| matches!(diagnostic.code.as_str(), "E0396" | "E0398")),
            "{diagnostics:#?}");
    }
}

#[test]
fn double_star_keeps_an_invalid_exponent_diagnostic_without_an_edit() {
    let source = "fn run() { print(2 **) }";
    let (tokens, _) = Lexer::lex(source);
    let diagnostics = Parser::parse_for_check_with_source(&tokens, source)
        .expect_err("double star and a missing exponent are rejected");
    let power = diagnostics.iter().find(|diagnostic| diagnostic.code == "E0396")
        .expect("power spelling is still diagnosed");
    assert!(power.edit.is_none(), "{power:#?}");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code != "E0396"),
        "the malformed exponent's diagnostic must not be discarded: {diagnostics:#?}");
}
