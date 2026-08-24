//! TIR subset/coverage gate (`tir_covers*` and `is_covered_*`/`*_in_subset` predicates).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

mod builtin_methods;
mod core_calls;
mod entry;
mod expressions;
mod handles;
mod methods;
mod patterns;
/// I2 self-report: which construct a `tir_covers*` refusal was about.
pub(crate) mod refusal;
mod statements;
mod structs_enums;
mod types;

pub(crate) use builtin_methods::*;
pub(crate) use core_calls::*;
pub(crate) use entry::*;
pub(crate) use expressions::*;
pub(crate) use handles::*;
pub(crate) use methods::*;
pub(crate) use patterns::*;
pub(crate) use statements::*;
pub(crate) use structs_enums::*;
pub(crate) use types::*;

// The civil-time `(receiver, method)` table is the one gate every tier asks,
// including the resident-JIT residency gate in the `jet-jit` crate (I8).
pub use builtin_methods::is_civil_time_method_name;
