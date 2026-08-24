//! TIR emission: TIR -> Rust source (`emit_tir_*`).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

mod core_calls;
mod encoding;
mod expressions;
mod functions;
mod helpers;
mod statements;

pub(crate) use core_calls::*;
pub(crate) use encoding::*;
pub(crate) use expressions::*;
pub(crate) use functions::*;
pub(crate) use helpers::*;
pub(crate) use statements::*;
