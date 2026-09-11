//! TIR lowering: AST -> TIR (`LowerEnv`, `lower_*`).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.


mod builtins;
mod call_args;
mod control_flow;
mod core_calls;
mod env;
mod expressions;
mod fields;
mod functions;
mod lambdas;
mod method_calls;
mod panic;
mod patterns;
mod statements;

pub(crate) use builtins::*;
pub(crate) use call_args::*;
pub(crate) use control_flow::*;
pub(crate) use core_calls::*;
pub(crate) use env::*;
pub(crate) use expressions::*;
pub(crate) use fields::*;
pub(crate) use functions::*;
pub(crate) use lambdas::*;
pub(crate) use method_calls::*;
pub(crate) use panic::*;
pub(crate) use patterns::*;
pub(crate) use statements::*;
