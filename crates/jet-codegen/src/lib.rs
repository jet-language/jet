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
pub mod local_cell {
    // The carrier is one type across every tier; the AOT copy of this file gets
    // it from the flat Prelude, this copy from jet-foundation.
    include!("Prelude/LocalCell.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// D-NUMWIDEN-CROSS1=E: one checked integer-to-float widening policy shared
/// by AOT emission, TIR evaluation, and the resident JIT adapter.
pub mod numeric_widen {
    include!("Prelude/NumericWiden.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// D-TASKGROUP-PARAM1=A: canonical structured task ownership policy. The JIT
/// compiles the same Prelude source that AOT embeds.
pub mod task_group {
    include!("Prelude/TaskGroup.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// D-TYPEDTEXT1=D: typed SQL/HTML/Sh semantics shared by AOT and TIR.
pub mod typed_text {
    include!("Prelude/TypedText.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// D-PROCESS-SESSION1=A / #1181: shared native Unix PTY substrate used by the
/// emitted process prelude and the resident JIT adapter.
#[path = "Prelude/CoreLib/ProcessPty.rs"]
pub mod process_pty;
// Prelude/ contains include_str-embedded text files, not Rust modules.
