//! Stable Jet CLI registry, argument vocabulary, help index, and rendering.
//!
//! D-ARCH-SOURCE1=A: these user-facing policies are one product seam. The
//! binary host dispatches through this crate; no command registry or help UI
//! remains in the root compiler package.

#![allow(non_snake_case)]
#![deny(warnings)]

pub use jet_foundation::Syntax;
pub use jet_repl::{SemanticSymbols, Term};

pub mod CLI;
pub mod Explain;
pub mod Help;
