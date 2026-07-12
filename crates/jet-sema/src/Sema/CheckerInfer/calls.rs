//! Type inference: calls, lambdas, method calls, and call checking.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

mod helpers_call_values;
mod lambdas;
mod builtin_methods;
mod options_rng;
mod method_calls;
mod direct_calls;
mod variadic;
mod helpers;
