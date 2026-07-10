#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export foundation so `crate::AST`, `crate::Syntax` etc. work in Comptime source files.
pub use jet_foundation::{BuildEffect, Collections, Diagnostics, Generics, Syntax, Traits, AST, SHA256};
pub mod Comptime;
