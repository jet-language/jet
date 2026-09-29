//! D-CAP-RECEIVER1=D: a line that starts with `&name` or `^name` begins a new
//! statement; a continued bitwise-and or power line keeps a space after its
//! operator, and `jet fmt` emits that space.

use jet::Lexer::TokKind;

fn semi_before(source: &str, mark: TokKind) -> bool {
    let (tokens, diagnostics) = jet::Lexer::lex(source);
    assert!(diagnostics.is_empty(), "{source:?}: {diagnostics:#?}");
    let at = tokens
        .iter()
        .position(|token| token.kind == mark)
        .unwrap_or_else(|| panic!("{source:?} has no {mark:?}"));
    at > 0 && tokens[at - 1].kind == TokKind::Semi
}

#[test]
fn line_start_place_marks_begin_statements() {
    assert!(semi_before("x\n&buf.push(1)\n", TokKind::Amp));
    assert!(semi_before("x\n^buf.seal()\n", TokKind::Caret));
    assert!(semi_before("x\n&self.items.push(1)\n", TokKind::Amp));
}

#[test]
fn spaced_operators_continue_the_previous_line() {
    assert!(!semi_before("x\n& mask\n", TokKind::Amp));
    assert!(!semi_before("x\n^ 2\n", TokKind::Caret));
    assert!(!semi_before("x\n&(mask)\n", TokKind::Amp));
}

#[test]
fn formatter_spaces_continued_operators_and_keeps_place_marks() {
    let source = "fn run() {\n    total := 6\n    mask := total\n        & 3\n    power := total\n        ^ 2\n    buf := [1]\n    &buf.push(2)\n    print(\"{mask} {power} {buf}\")\n}\n";
    let formatted = jet::format_source(source).expect("receiver marks format");
    assert!(formatted.contains("mask := total & 3"), "{formatted}");
    assert!(formatted.contains("power := total ^ 2"), "{formatted}");
    assert!(formatted.contains("    &buf.push(2)\n"), "{formatted}");
    assert_eq!(
        jet::format_source(&formatted).expect("formatted source is stable"),
        formatted
    );
}
