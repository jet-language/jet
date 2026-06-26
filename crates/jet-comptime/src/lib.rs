#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export foundation so `crate::AST`, `crate::Syntax` etc. work in Comptime source files.
pub use jet_foundation::{AST, Collections, Diagnostics, Generics, SHA256, Syntax, Traits};
pub mod Comptime;
