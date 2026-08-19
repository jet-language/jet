#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export foundation so `crate::AST`, `crate::Syntax` etc. work in all Lexer source files.
pub use jet_foundation::{
    CanonicalAST, Collections, Diagnostics, Generics, Numeric, Policy, Syntax, TargetMachine,
    Traits, AST, SHA256,
};
pub mod Lexer;

#[cfg(test)]
mod compare_tests {
    use super::Lexer::{self, TokKind};

    #[test]
    fn spaceship_is_one_longest_match_before_le() {
        let (tokens, diagnostics) = Lexer::lex("a <=> b <= c");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let code = Lexer::without_comments(&tokens);
        assert!(matches!(&code[1].kind, TokKind::Compare));
        assert_eq!(code[1].span.end - code[1].span.start, 3);
        assert!(matches!(&code[3].kind, TokKind::Le));
        assert_eq!(code[3].span.end - code[3].span.start, 2);
    }
}

#[cfg(test)]
mod raw_head_tests {
    use super::Lexer::{self, StrTokPart, TokKind};

    fn first_string_lit(src: &str) -> String {
        let (tokens, diagnostics) = Lexer::lex(src);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        for token in Lexer::without_comments(&tokens) {
            if let TokKind::Str(parts) = token.kind {
                match parts.as_slice() {
                    [StrTokPart::Lit(text)] => return text.clone(),
                    _ => panic!("expected one literal string part, got {parts:?}"),
                }
            }
        }
        panic!("no string token in {src:?}");
    }

    #[test]
    fn typed_head_keeps_backslash_literal() {
        assert_eq!(first_string_lit(r#"Regex.{"\d+"}"#), r"\d+");
        assert_eq!(first_string_lit(r#"Regex.{"a\nb"}"#), r"a\nb");
        assert_eq!(first_string_lit("\"a\\nb\""), "a\nb");
    }

    #[test]
    fn typed_head_quote_stays_in_body_when_slashed() {
        assert_eq!(first_string_lit(r#"Regex.{"a\"b"}"#), r#"a\"b"#);
    }

    #[test]
    fn typed_head_doubled_braces_stay_literal() {
        assert_eq!(first_string_lit(r#"Regex.{"\d{{2}}"}"#), r"\d{2}");
    }

    #[test]
    fn typed_head_holes_still_split_interpolation() {
        let (tokens, diagnostics) = Lexer::lex(r#"URL.{"https://x/{name}"}"#);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let parts = Lexer::without_comments(&tokens)
            .into_iter()
            .find_map(|token| match token.kind {
                TokKind::Str(parts) => Some(parts),
                _ => None,
            })
            .expect("typed head string");
        assert!(
            matches!(
                parts.as_slice(),
                [StrTokPart::Lit(prefix), StrTokPart::Interp(_)]
                    if prefix == "https://x/"
            ),
            "{parts:?}"
        );
    }

    #[test]
    fn plain_string_still_rejects_unknown_escapes() {
        let (_tokens, diagnostics) = Lexer::lex(r#""C:\path""#);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.what.contains("isn't an escape")),
            "{diagnostics:?}"
        );
    }
}
