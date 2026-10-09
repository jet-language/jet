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
