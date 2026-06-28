#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams. Sema transitively includes Parser/Lexer/Comptime/Foundation.
pub use jet_sema::{
    Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Sema, Syntax, Traits,
    AST, SHA256,
};
pub mod Codegen;
// Prelude/ contains include_str-embedded text files, not Rust modules.
