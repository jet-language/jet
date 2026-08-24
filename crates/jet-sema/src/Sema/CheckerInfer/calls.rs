//! Type inference: calls, lambdas, method calls, and call checking.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

mod builtin_methods;
mod direct_calls;
mod helpers;
mod helpers_call_values;
mod lambdas;
mod method_calls;
mod options_rng;
mod variadic;
