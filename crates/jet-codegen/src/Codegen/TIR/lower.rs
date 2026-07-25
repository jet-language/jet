//! TIR lowering: AST -> TIR (`LowerEnv`, `lower_*`, render helpers).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

use std::cell::Cell;

thread_local! {
    /// Set while `#777` fragment eval lowers AST without full sema method facts.
    static EVAL_FRAGMENT: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` with eval-fragment lowering (unknown methods stay MethodCall, not Todo).
pub(crate) fn with_eval_fragment<R>(f: impl FnOnce() -> R) -> R {
    EVAL_FRAGMENT.with(|flag| {
        let prev = flag.replace(true);
        let out = f();
        flag.set(prev);
        out
    })
}

pub(crate) fn is_eval_fragment() -> bool {
    EVAL_FRAGMENT.with(Cell::get)
}

mod env;
mod functions;
mod statements;
mod control_flow;
mod patterns;
mod panic;
mod expressions;
mod fields;
mod method_calls;
mod core_calls;
mod call_args;
mod builtins;
mod lambdas;

pub(crate) use env::*;
pub(crate) use functions::*;
pub(crate) use statements::*;
pub(crate) use control_flow::*;
pub(crate) use patterns::*;
pub(crate) use panic::*;
pub(crate) use expressions::*;
pub(crate) use fields::*;
pub(crate) use method_calls::*;
pub(crate) use core_calls::*;
pub(crate) use call_args::*;
pub(crate) use builtins::*;
pub(crate) use lambdas::*;
