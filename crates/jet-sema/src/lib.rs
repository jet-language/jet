#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams so `crate::AST`, `crate::Lexer`, `crate::Parser`, `crate::Comptime`
// all work within Sema source files without cross-crate path changes.
pub use jet_parser::{AST, Collections, Diagnostics, Formatter, Generics, Lexer, Parser, SHA256, Syntax, Traits};
pub use jet_comptime::Comptime;
pub mod Sema;
