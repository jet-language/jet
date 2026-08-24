#![allow(non_snake_case)]
#![deny(warnings)]
// #777: AST-era host helpers (Methods/TypedDecode/…) stay until TIR arms absorb
// every ambient call; the live path is TirBridge → TIR eval → apply_*.
#![allow(dead_code)]
// Re-export foundation so `crate::AST`, `crate::Syntax` etc. work in Comptime source files.
pub use jet_foundation::{
    BuildEffect, Collections, Diagnostics, Generics, Numeric, Syntax, Traits, AST, SHA256,
};

// The included `jet_std` Prelude fragments resolve exact-number lexing through
// `crate::jet_json_number`, the same path `jet-jit` provides at its own root.
#[allow(unused_imports)]
pub(crate) use jet_foundation::JSONNumber as jet_json_number;

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
