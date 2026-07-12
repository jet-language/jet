//! TIR emission: TIR -> Rust source (`emit_tir_*`).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

mod functions;
mod statements;
mod expressions;
mod encoding;
mod core_calls;
mod helpers;

pub(crate) use functions::*;
pub(crate) use statements::*;
pub(crate) use expressions::*;
pub(crate) use encoding::*;
pub(crate) use core_calls::*;
pub(crate) use helpers::*;
