#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams so `crate::AST`, `crate::Lexer`, `crate::Parser`, `crate::Comptime`
// all work within Sema source files without cross-crate path changes.
pub use jet_comptime::Comptime;
pub use jet_parser::{
    Collections, Diagnostics, Formatter, Generics, Lexer, Parser, Syntax, Traits, AST, SHA256,
};
pub mod Sema;
pub use Sema::{effect_key, SemIndexEffectFacts};
