#![allow(non_snake_case)]
#![deny(warnings)]
// #777: AST-era host helpers (Methods/TypedDecode/…) stay until TIR arms absorb
// every ambient call; the live path is TirBridge → TIR eval → apply_*.
#![allow(dead_code)]
// Re-export foundation so `crate::AST`, `crate::Syntax` etc. work in Comptime source files.
pub use jet_foundation::{
    BuildEffect, Collections, Diagnostics, Generics, Numeric, Syntax, Traits, AST,
    SHA256,
};

pub(crate) trait JetShow {
    fn jet_show(&self) -> String;
}

/// Display/debug seams for included Prelude fragments (`impl crate::JetDisplay`).
pub(crate) trait JetDisplay {
    fn jet_display(&self) -> String;
}

pub(crate) trait JetDebug {
    fn jet_debug(&self) -> String;
}

pub mod Comptime;
