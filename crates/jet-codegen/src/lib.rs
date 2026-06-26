#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams. Sema transitively includes Parser/Lexer/Comptime/Foundation.
pub use jet_sema::{AST, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Sema, SHA256, Syntax, Traits};
pub mod Codegen;
// Prelude/ contains include_str-embedded text files, not Rust modules.
