#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export foundation so `crate::AST`, `crate::Syntax` etc. work in all Lexer source files.
pub use jet_foundation::{Collections, Diagnostics, Generics, Syntax, Traits, AST, SHA256};
pub mod Lexer;
