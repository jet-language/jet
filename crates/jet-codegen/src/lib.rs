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
/// D-TASKGROUP-PARAM1=A: canonical structured task ownership policy. The JIT
/// compiles the same Prelude source that AOT embeds.
#[path = "Prelude/TaskGroup.rs"]
pub mod task_group;
// Prelude/ contains include_str-embedded text files, not Rust modules.
