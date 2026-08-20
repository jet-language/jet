//! TIR subset/coverage gate (`tir_covers*` and `is_covered_*`/`*_in_subset` predicates).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

mod entry;
mod types;
mod structs_enums;
mod statements;
mod patterns;
mod expressions;
mod methods;
mod builtin_methods;
mod core_calls;
mod handles;
/// I2 self-report: which construct a `tir_covers*` refusal was about.
pub(crate) mod refusal;

pub(crate) use entry::*;
pub(crate) use types::*;
pub(crate) use structs_enums::*;
pub(crate) use statements::*;
pub(crate) use patterns::*;
pub(crate) use expressions::*;
pub(crate) use methods::*;
pub(crate) use builtin_methods::*;
pub(crate) use core_calls::*;
pub(crate) use handles::*;

// The civil-time `(receiver, method)` table is the one gate every tier asks,
// including the resident-JIT residency gate in the `jet-jit` crate (I8).
pub use builtin_methods::is_civil_time_method_name;
