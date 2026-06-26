#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export foundation + lexer so `crate::AST`, `crate::Lexer` etc. work in Parser/Formatter.
pub use jet_lexer::{AST, Collections, Diagnostics, Generics, Lexer, SHA256, Syntax, Traits};
pub mod Formatter;
pub mod Parser;
