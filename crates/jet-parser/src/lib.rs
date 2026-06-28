#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export foundation + lexer so `crate::AST`, `crate::Lexer` etc. work in Parser/Formatter.
pub use jet_lexer::{Collections, Diagnostics, Generics, Lexer, Syntax, Traits, AST, SHA256};
pub mod Formatter;
pub mod Parser;
