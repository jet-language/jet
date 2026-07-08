//! TIR emission: TIR -> Rust source (`emit_tir_*`).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

use super::*;
use crate::AST::{AccessConvention, BinOp, Type, UnOp};

include!("emit/functions.rs");
include!("emit/statements.rs");
include!("emit/expressions.rs");
include!("emit/encoding.rs");
include!("emit/core_calls.rs");
include!("emit/helpers.rs");
