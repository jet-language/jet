//! c46 drift-guard: TextMate grammar keyword patterns must track Syntax::JET_KEYWORD_LIST.
//!
//! Reads `editors/vscode/syntaxes/jet.tmLanguage.json`, extracts every word
//! from the keyword `"match"` regex alternation groups, and asserts:
//!   1. Every word in `Syntax::JET_KEYWORD_LIST` is covered (no silent omissions).
//!   2. No `FOREIGN_*` word from `crates/jet-foundation/src/Syntax.rs` appears in a keyword pattern
//!      (those are teaching-error-only words that must not get syntax highlighting).
//!
//! The test does NOT rewrite the grammar — it is a tripwire so that a keyword
//! addition that wasn't mirrored in the grammar fails loudly.

use std::collections::BTreeSet;
use std::fs;

/// Extract every literal word from `\b(word1|word2|...)\b`-style match patterns
/// in the grammar's keywords section.
///
/// Strategy: parse line by line; look for `"match":` lines inside the keywords
/// block and extract the alternation content between `\b(` and `)\b`.
fn extract_grammar_keyword_words(grammar_text: &str) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    let mut in_keywords = false;
    let mut depth = 0i32;

    for line in grammar_text.lines() {
        let trimmed = line.trim();

        // Enter the "keywords" object.
        if trimmed.contains("\"keywords\"") {
            in_keywords = true;
        }
        if in_keywords {
            depth += trimmed.chars().filter(|&c| c == '{').count() as i32;
            depth -= trimmed.chars().filter(|&c| c == '}').count() as i32;
            if depth <= 0 && in_keywords && !trimmed.contains("\"keywords\"") {
                in_keywords = false;
            }

            // Find `"match": "...\b(...)\b..."` lines.
            if let Some(match_start) = trimmed.find("\"match\"") {
                let after = &trimmed[match_start..];
                // The value is a JSON string after the colon.
                if let Some(val_start) = after.find('"').and_then(|_| {
                    let rest = &after[after.find(':').unwrap_or(0) + 1..];
                    let rest = rest.trim_start();
                    if rest.starts_with('"') {
                        Some(rest)
                    } else {
                        None
                    }
                }) {
                    // val_start is the string value (starts with `"`).
                    let inner = &val_start[1..];
                    if let Some(end) = inner.rfind('"') {
                        let pattern = &inner[..end];
                        // Extract alternation groups: content between `\b(` and `)\b`
                        // or just `(` and `)` for patterns without word boundaries.
                        extract_alternation_words(pattern, &mut words);
                    }
                }
            }
        }
    }
    words
}

/// Pull words out of `\b(a|b|c)\b` or `(a|b|c)` patterns.
fn extract_alternation_words(pattern: &str, out: &mut BTreeSet<String>) {
    // Find all `(...)` groups and split on `|`.
    let mut rest = pattern;
    while let Some(open) = rest.find('(') {
        let inner_start = open + 1;
        if let Some(close) = rest[inner_start..].find(')') {
            let group = &rest[inner_start..inner_start + close];
            // Only process groups that look like alternations of simple words
            // (no nested parens, no backslashes inside).
            if !group.contains('(') && !group.contains('\\') {
                for word in group.split('|') {
                    let w = word.trim();
                    if !w.is_empty() && w.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        out.insert(w.to_string());
                    }
                }
            }
            rest = &rest[inner_start + close + 1..];
        } else {
            break;
        }
    }
}

/// Collect FOREIGN_* constant values from crates/jet-foundation/src/Syntax.rs by scanning for
/// lines like `pub const FOREIGN_XXX: &str = "word";`.
fn extract_foreign_words_from_syntax(syntax_text: &str) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    for line in syntax_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub const FOREIGN_") {
            // Extract the value between the last `"` pair.
            if let Some(start) = trimmed.rfind('"') {
                let before = &trimmed[..start];
                if let Some(end) = before.rfind('"') {
                    let word = &before[end + 1..];
                    // Skip multi-character sigils / operators and multi-word strings.
                    if !word.is_empty()
                        && !word.contains(' ')
                        && word.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        words.insert(word.to_string());
                    }
                }
            }
        }
    }
    words
}

#[test]
fn grammar_covers_jet_keyword_list() {
    let grammar_path = "editors/vscode/syntaxes/jet.tmLanguage.json";
    let grammar_text =
        fs::read_to_string(grammar_path).expect("editors/vscode/syntaxes/jet.tmLanguage.json");

    let grammar_words = extract_grammar_keyword_words(&grammar_text);

    // JET_KEYWORD_LIST values — kept in sync with crates/jet-foundation/src/Syntax.rs constants.
    // This list must be updated whenever JET_KEYWORD_LIST changes.
    let jet_keywords: BTreeSet<&str> = jet::Syntax::JET_KEYWORD_LIST.iter().copied().collect();

    let missing: Vec<&&str> = jet_keywords
        .iter()
        .filter(|kw| {
            let w = kw.to_string();
            !grammar_words.contains(&w)
        })
        .collect();

    assert!(
        missing.is_empty(),
        "Grammar file `{grammar_path}` is missing keyword(s) from JET_KEYWORD_LIST: {:?}\n\
         Add them to the appropriate \\b(...)\\b pattern in the keywords section.",
        missing
    );
}

#[test]
fn grammar_excludes_foreign_words() {
    let grammar_path = "editors/vscode/syntaxes/jet.tmLanguage.json";
    let grammar_text =
        fs::read_to_string(grammar_path).expect("editors/vscode/syntaxes/jet.tmLanguage.json");
    let syntax_text = fs::read_to_string("crates/jet-foundation/src/Syntax.rs")
        .expect("crates/jet-foundation/src/Syntax.rs");

    let grammar_words = extract_grammar_keyword_words(&grammar_text);
    let foreign_words = extract_foreign_words_from_syntax(&syntax_text);

    // JET_KEYWORD_LIST words are allowed even if their constant is FOREIGN_*-named —
    // but in practice no FOREIGN_* constant is in JET_KEYWORD_LIST.
    let jet_keywords: BTreeSet<&str> = jet::Syntax::JET_KEYWORD_LIST.iter().copied().collect();

    let intruders: Vec<String> = foreign_words
        .iter()
        .filter(|w| {
            // Present in grammar keywords AND not in the real keyword list.
            grammar_words.contains(*w) && !jet_keywords.contains(w.as_str())
        })
        .cloned()
        .collect();

    assert!(
        intruders.is_empty(),
        "Grammar file `{grammar_path}` contains FOREIGN_* teaching-error word(s) in its keyword patterns: {:?}\n\
         These words must NOT be highlighted as keywords — remove them from the match pattern.",
        intruders
    );
}

// ============================================================================
// Section: lexer smoke (was tests/m3_lexer_smoke.rs)
// ============================================================================

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
    assert_eq!(count_semis("fn main() {\n    x :: 1\n    y :: 2\n}\n"), 2);

    // Dot-chain continuation (S69): a leading `.` suppresses insertion, so the
    // whole `a.b()` is one statement → just the pre-`}` terminator.
    assert_eq!(count_semis("fn main() {\n    a()\n        .b()\n}\n"), 1);

    // Broken boolean: a leading `&&` suppresses insertion → one statement.
    assert_eq!(count_semis("fn main() {\n    x :: a\n        && b\n}\n"), 1);

    // Broken arithmetic: a leading `+` suppresses insertion → one statement.
    assert_eq!(count_semis("fn main() {\n    x :: a\n        + b\n}\n"), 1);

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

// ============================================================================
// Section: parser smoke (was tests/m3_parse_smoke.rs)
// ============================================================================

#[test]
fn parse_option_fn() {
    let src = r#"
fn find_even(limit: Int) -> (Int?) {
    loop i in 1..limit {
        if i % 2 == 0 {
            return value(i);
        }
    }
    return null;
}
fn main() {}
"#;
    let (toks, _) = jet::Lexer::lex(src);
    let prog = jet::Parser::parse(&toks).expect("parse");
    assert_eq!(prog.items.len(), 2);
}

#[test]
fn parse_pipe_switch_arms_as_subject_tests() {
    // D-IF3: explicit `if subject == { … }` dispatch. A bare value arm becomes
    // `subject == value`; a `||` chain of bare values re-applies the comparison
    // to each (`orange || mango` ≡ `fruit == orange || fruit == mango`).
    let src = r#"
fn main() {
    fruit :: "orange"
    if fruit == {
        apple -> { print("Apple Juice") }
        orange || mango -> { print("Orange Juice") }
        tangerine || yuzu -> { print("Citrus Juice") }
        else -> { print("Water") }
    }
}
"#;
    let (toks, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    let prog = jet::Parser::parse(&toks).expect("parse");
    let jet::AST::Item::Func(func) = &prog.items[0] else {
        panic!("expected function");
    };
    let jet::AST::Stmt::Switch {
        subject,
        arms,
        else_body,
        ..
    } = &func.body[1]
    else {
        panic!("expected switch");
    };
    assert!(matches!(subject, jet::AST::Expr::Ident(name, _) if name == "fruit"));
    assert_eq!(arms.len(), 3);
    assert!(else_body.is_some());
    assert!(matches!(
        &arms[0].cond,
        jet::AST::Expr::Binary(jet::AST::BinOp::Eq, _, _, _)
    ));
    assert!(matches!(
        &arms[1].cond,
        jet::AST::Expr::Binary(jet::AST::BinOp::Or, _, _, _)
    ));
}

#[test]
fn parse_bracket_collection_types_and_semicolon_list_items() {
    let src = r#"
pub fn shell() -> [JSON] {
    return [
        JSON.Null;
    ];
}

fn use_collections(items: [String], counts: [String, Int]) {}
"#;
    let (toks, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    let prog = jet::Parser::parse(&toks).expect("parse");
    let jet::AST::Item::Func(shell) = &prog.items[0] else {
        panic!("expected shell function");
    };
    assert!(matches!(shell.return_type, Some(jet::AST::Type::List(_))));
    let jet::AST::Item::Func(use_collections) = &prog.items[1] else {
        panic!("expected use_collections function");
    };
    assert!(matches!(
        use_collections.params[0].ty,
        jet::AST::Type::List(_)
    ));
    assert!(matches!(
        use_collections.params[1].ty,
        jet::AST::Type::Map { .. }
    ));
}

#[test]
fn parse_numeric_field_emits_e0049() {
    let src = "fn main() { x :: p.0 }";
    let (toks, _) = jet::Lexer::lex(src);
    let (_prog, diags) = jet::Parser::parse_for_check(&toks).expect("recoverable parse");
    assert!(
        diags.iter().any(|d| d.code == "E0049"),
        "expected E0049 for `.0` access, got: {diags:?}"
    );
}
