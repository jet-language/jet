#[test]
fn enum_is_keyword() {
    let (toks, diags) = jet::Lexer::lex("enum x { A; }");
    assert!(diags.is_empty(), "{diags:?}");
    assert!(
        matches!(toks[0].kind, jet::Lexer::TokKind::KwEnum),
        "{:?}",
        toks[0].kind
    );
}

/// S6-R: the lexer inserts a synthetic statement terminator at line ends after
/// a statement-ending token, with continuation suppression. Pins the four cases
/// the design doc calls out: dot-chain, broken boolean, broken arithmetic, and
/// the negative case (a genuine statement on the next line gets a terminator).
#[test]
fn s6r_terminator_insertion_and_suppression() {
    use jet::Lexer::TokKind;
    let count_semis = |src: &str| -> usize {
        let (toks, diags) = jet::Lexer::lex(src);
        assert!(diags.is_empty(), "lex diags for {src:?}: {diags:?}");
        toks.iter()
            .filter(|t| matches!(t.kind, TokKind::Semi))
            .count()
    };

    // Negative case: two real statements → a terminator between them (+ one
    // before the closing `}`), so 2 synthetic terminators inside the body.
    assert_eq!(count_semis("fn main() {\n    x @= 1\n    y @= 2\n}\n"), 2);

    // Dot-chain continuation (S69): a leading `.` suppresses insertion, so the
    // whole `a.b()` is one statement → just the pre-`}` terminator.
    assert_eq!(count_semis("fn main() {\n    a()\n        .b()\n}\n"), 1);

    // Broken boolean: a leading `&&` suppresses insertion → one statement.
    assert_eq!(count_semis("fn main() {\n    x @= a\n        && b\n}\n"), 1);

    // Broken arithmetic: a leading `+` suppresses insertion → one statement.
    assert_eq!(count_semis("fn main() {\n    x @= a\n        + b\n}\n"), 1);

    // A closing `)` on its own line never gets a terminator before it
    // (multi-line call args) → one statement.
    assert_eq!(count_semis("fn main() {\n    f(\n        a\n    )\n}\n"), 1);
}

#[test]
fn dot_zero_in_statement_lexes_as_dot_then_int() {
    let (toks, diags) = jet::Lexer::lex("fn main() { val x = p.0; }");
    assert!(diags.is_empty(), "{diags:?}");
    let dot = toks
        .iter()
        .position(|t| matches!(t.kind, jet::Lexer::TokKind::Dot))
        .expect("dot");
    assert!(
        matches!(toks[dot + 1].kind, jet::Lexer::TokKind::Int(0)),
        "{:?}",
        toks[dot + 1].kind
    );
}
