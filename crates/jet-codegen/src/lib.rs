#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams. Sema transitively includes Parser/Lexer/Comptime/Foundation.
pub use jet_sema::{
    CanonicalAST, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Sema,
    Syntax, TargetProfile, Traits, AST, SHA256,
};
pub mod Codegen;
mod BrowserHost;
/// D-ASYNCRT1=A: M:N scheduler substrate for jet-jit host shims.
pub mod scheduler;
/// D-LOCALCELL1=A: canonical local Cell runtime shared by emitted AOT code and
/// the TIR evaluator's deopt adapter.
#[path = "Prelude/LocalCell.rs"]
pub mod local_cell;
/// D-NUMWIDEN-CROSS1=E: one checked integer-to-float widening policy shared
/// by AOT emission, TIR evaluation, and the resident JIT adapter.
#[path = "Prelude/NumericWiden.rs"]
pub mod numeric_widen;
/// D-TASKGROUP-PARAM1=A: canonical structured task ownership policy. The JIT
/// compiles the same Prelude source that AOT embeds.
#[path = "Prelude/TaskGroup.rs"]
pub mod task_group;
// Prelude/ contains include_str-embedded text files, not Rust modules.
