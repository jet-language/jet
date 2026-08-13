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
